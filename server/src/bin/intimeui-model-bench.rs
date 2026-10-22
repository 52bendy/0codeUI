//! 模型推理延迟 / 成本基线测量 CLI。
//!
//! 用法：
//! ```bash
//! # mock 基线（无需 GPU）
//! cargo run -p intimeui-server --bin intimeui-model-bench -- \
//!   --requests 100 --concurrency 4 --output /tmp/model-bench.json
//!
//! # 真实模型（先按 docs/model-inference-deployment.md 启动 vLLM）
//! MODEL_PROVIDER=openai_compatible \
//! MODEL_BASE_URL=http://127.0.0.1:8000/v1 \
//! MODEL_NAME=Qwen2.5-Coder-7B-Instruct-AWQ \
//! cargo run --release -p intimeui-server --bin intimeui-model-bench -- \
//!   --requests 100 --concurrency 4 --markdown docs/model-latency-baseline.md
//! ```

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use intimeui_server::model::{ModelConfig, ModelGateway, UiRequest};
use serde::Serialize;

const DEFAULT_PROMPT: &str =
    "生成一个订单详情页：订单号 ORD_20261014_001，状态 PAID，商品「智能降噪耳机」299 元 1 件；黑白灰主题，支持明暗切换。";

#[derive(Debug, Clone)]
struct Args {
    requests: usize,
    concurrency: usize,
    prompt: String,
    output: Option<String>,
    markdown: Option<String>,
}

#[derive(Debug, Serialize)]
struct BenchReport {
    generated_at: String,
    provider: String,
    primary_model: String,
    requests: usize,
    concurrency: usize,
    success: usize,
    failures: usize,
    html_valid: usize,
    latency_ms: LatencyStats,
    throughput_rps: f64,
    input_tokens: u64,
    output_tokens: u64,
    estimated_cost_usd: f64,
    note: String,
}

#[derive(Debug, Serialize)]
struct LatencyStats {
    min: f64,
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    let config = match ModelConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let gateway = match ModelGateway::from_config(&config) {
        Ok(gateway) => Arc::new(gateway),
        Err(error) => {
            eprintln!("failed to initialize model gateway: {error}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    for worker in 0..args.concurrency {
        let gateway = gateway.clone();
        let prompt = args.prompt.clone();
        let count = args.requests / args.concurrency
            + usize::from(worker < args.requests % args.concurrency);
        tasks.spawn(async move { run_worker(gateway, prompt, count).await });
    }

    let mut success = 0usize;
    let mut failures = 0usize;
    let mut latencies = Vec::new();
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut cost = 0.0f64;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(worker) => {
                success += worker.success;
                failures += worker.failures;
                latencies.extend(worker.latencies);
                input_tokens += worker.input_tokens;
                output_tokens += worker.output_tokens;
                cost += worker.cost;
            }
            Err(error) => {
                failures += 1;
                eprintln!("worker failed: {error}");
            }
        }
    }
    let wall_secs = started.elapsed().as_secs_f64().max(0.001);
    latencies.sort_by(f64::total_cmp);
    let report = BenchReport {
        generated_at: Utc::now().to_rfc3339(),
        provider: gateway.info().provider.to_string(),
        primary_model: gateway.info().primary_model,
        requests: args.requests,
        concurrency: args.concurrency,
        success,
        failures,
        html_valid: success,
        latency_ms: latency_stats(&latencies),
        throughput_rps: round3(success as f64 / wall_secs),
        input_tokens,
        output_tokens,
        estimated_cost_usd: round6(cost),
        note: "html_valid 由网关的单文件 HTML 校验得出；mock 数据仅用于验证测量链路，真实模型基线需在 24GB GPU 环境运行。".to_string(),
    };

    let json = match serde_json::to_string_pretty(&report) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("failed to serialize report: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{json}");
    if let Some(path) = &args.output {
        if let Err(error) = std::fs::write(path, &json) {
            eprintln!("failed to write {path}: {error}");
            return ExitCode::FAILURE;
        }
    }
    if let Some(path) = &args.markdown {
        if let Err(error) = std::fs::write(path, render_markdown(&report)) {
            eprintln!("failed to write {path}: {error}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

struct WorkerResult {
    success: usize,
    failures: usize,
    latencies: Vec<f64>,
    input_tokens: u64,
    output_tokens: u64,
    cost: f64,
}

async fn run_worker(gateway: Arc<ModelGateway>, prompt: String, count: usize) -> WorkerResult {
    let mut result = WorkerResult {
        success: 0,
        failures: 0,
        latencies: Vec::new(),
        input_tokens: 0,
        output_tokens: 0,
        cost: 0.0,
    };
    for _ in 0..count {
        let request = UiRequest {
            prompt: prompt.clone(),
            scene_code: Some("ORDER_DETAIL_PAGE".to_string()),
            model: None,
            max_tokens: None,
            temperature: None,
        };
        match gateway.generate_ui(request).await {
            Ok(generation) => {
                result.success += 1;
                result.latencies.push(generation.latency_ms);
                result.input_tokens += u64::from(generation.usage.prompt_tokens);
                result.output_tokens += u64::from(generation.usage.completion_tokens);
                result.cost += generation.estimated_cost_usd;
            }
            Err(error) => {
                result.failures += 1;
                eprintln!("request failed: {error}");
            }
        }
    }
    result
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        requests: 20,
        concurrency: 1,
        prompt: DEFAULT_PROMPT.to_string(),
        output: None,
        markdown: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--requests" => args.requests = parse_usize(&mut iter, "--requests")?,
            "--concurrency" => args.concurrency = parse_usize(&mut iter, "--concurrency")?,
            "--prompt" => args.prompt = next_value(&mut iter, "--prompt")?,
            "--output" => args.output = Some(next_value(&mut iter, "--output")?),
            "--markdown" => args.markdown = Some(next_value(&mut iter, "--markdown")?),
            "--help" | "-h" => {
                return Err("usage: intimeui-model-bench [--requests N] [--concurrency N] [--prompt TEXT] [--output FILE] [--markdown FILE]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if args.requests == 0 {
        return Err("--requests must be >= 1".to_string());
    }
    if args.concurrency == 0 {
        return Err("--concurrency must be >= 1".to_string());
    }
    if args.concurrency > args.requests {
        args.concurrency = args.requests;
    }
    Ok(args)
}

fn next_value(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    iter.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_usize(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<usize, String> {
    let value = next_value(iter, flag)?;
    value
        .parse()
        .map_err(|_| format!("{flag} must be a positive integer"))
}

fn latency_stats(sorted: &[f64]) -> LatencyStats {
    if sorted.is_empty() {
        return LatencyStats {
            min: 0.0,
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
        };
    }
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    LatencyStats {
        min: round3(sorted[0]),
        mean: round3(mean),
        p50: round3(percentile(sorted, 0.50)),
        p95: round3(percentile(sorted, 0.95)),
        p99: round3(percentile(sorted, 0.99)),
        max: round3(sorted[sorted.len() - 1]),
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    let index = ((sorted.len() as f64 * quantile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

fn render_markdown(report: &BenchReport) -> String {
    format!(
        "# 模型推理延迟基线\n\n\
         > 由 `intimeui-model-bench` 自动生成，方法见 `docs/model-inference-deployment.md` §6。\n\n\
         | 项目 | 值 |\n| --- | --- |\n\
         | 生成时间 | {} |\n| 提供者 | {} |\n| 主模型 | {} |\n| 请求数 / 并发 | {} / {} |\n\
         | 成功 / 失败 | {} / {} |\n| 单文件 HTML 合法 | {} |\n\
         | P50 / P95 / P99 | {} / {} / {} ms |\n| 平均 / 最小 / 最大 | {} / {} / {} ms |\n\
         | 吞吐 | {} req/s |\n| 输入 / 输出 token | {} / {} |\n| 估算成本 | ${} |\n\n\
         {}\n",
        report.generated_at,
        report.provider,
        report.primary_model,
        report.requests,
        report.concurrency,
        report.success,
        report.failures,
        report.html_valid,
        report.latency_ms.p50,
        report.latency_ms.p95,
        report.latency_ms.p99,
        report.latency_ms.mean,
        report.latency_ms.min,
        report.latency_ms.max,
        report.throughput_rps,
        report.input_tokens,
        report.output_tokens,
        report.estimated_cost_usd,
        report.note,
    )
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}
