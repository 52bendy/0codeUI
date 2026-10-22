//! 进程内模型调用指标：请求量、成功率、延迟分位与 token / 成本估算。
//!
//! 单机内存实现，重启即清零；生产可在此基础上接 Prometheus exporter。

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde::Serialize;

use super::types::Usage;

const DEFAULT_WINDOW: usize = 2048;

/// 单次调用样本。
#[derive(Debug, Clone)]
struct Sample {
    model: String,
    latency_ms: f64,
    success: bool,
}

#[derive(Debug, Default)]
struct ModelCounters {
    requests: u64,
    success: u64,
    failures: u64,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
}

#[derive(Debug, Default)]
struct MetricsInner {
    total_requests: u64,
    success_requests: u64,
    failed_requests: u64,
    retries: u64,
    queue_rejections: u64,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    samples: VecDeque<Sample>,
    by_model: BTreeMap<String, ModelCounters>,
}

/// 指标快照（可直接序列化返回）。
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    /// 总请求数。
    pub total_requests: u64,
    /// 成功请求数。
    pub success_requests: u64,
    /// 失败请求数。
    pub failed_requests: u64,
    /// 重试次数。
    pub retries: u64,
    /// 因排队被拒绝的请求数。
    pub queue_rejections: u64,
    /// 平均延迟（毫秒）。
    pub avg_latency_ms: f64,
    /// P50 延迟（毫秒）。
    pub p50_latency_ms: f64,
    /// P95 延迟（毫秒）。
    pub p95_latency_ms: f64,
    /// P99 延迟（毫秒）。
    pub p99_latency_ms: f64,
    /// 输入 token 总数。
    pub input_tokens: u64,
    /// 输出 token 总数。
    pub output_tokens: u64,
    /// 估算成本（美元）。
    pub estimated_cost_usd: f64,
    /// 分模型统计。
    pub by_model: Vec<ModelMetric>,
}

/// 单个模型的指标。
#[derive(Debug, Clone, Serialize)]
pub struct ModelMetric {
    /// 模型名。
    pub model: String,
    /// 请求数。
    pub requests: u64,
    /// 成功数。
    pub success: u64,
    /// 失败数。
    pub failures: u64,
    /// 平均延迟（毫秒）。
    pub avg_latency_ms: f64,
    /// P95 延迟（毫秒）。
    pub p95_latency_ms: f64,
    /// 输入 token。
    pub input_tokens: u64,
    /// 输出 token。
    pub output_tokens: u64,
    /// 估算成本（美元）。
    pub estimated_cost_usd: f64,
}

/// 线程安全的模型指标收集器。
#[derive(Debug)]
pub struct ModelMetrics {
    inner: Mutex<MetricsInner>,
    window: usize,
}

impl Default for ModelMetrics {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOW)
    }
}

impl ModelMetrics {
    /// 指定滑动窗口大小。
    pub fn new(window: usize) -> Self {
        Self {
            inner: Mutex::new(MetricsInner::default()),
            window: window.max(1),
        }
    }

    /// 记录一次调用。
    pub fn record(&self, model: &str, latency_ms: f64, success: bool, usage: Usage, cost_usd: f64) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        inner.total_requests += 1;
        if success {
            inner.success_requests += 1;
        } else {
            inner.failed_requests += 1;
        }
        inner.input_tokens += u64::from(usage.prompt_tokens);
        inner.output_tokens += u64::from(usage.completion_tokens);
        inner.cost_usd += cost_usd;
        let counters = inner.by_model.entry(model.to_string()).or_default();
        counters.requests += 1;
        if success {
            counters.success += 1;
        } else {
            counters.failures += 1;
        }
        counters.input_tokens += u64::from(usage.prompt_tokens);
        counters.output_tokens += u64::from(usage.completion_tokens);
        counters.cost_usd += cost_usd;
        if inner.samples.len() >= self.window {
            inner.samples.pop_front();
        }
        inner.samples.push_back(Sample {
            model: model.to_string(),
            latency_ms,
            success,
        });
    }

    /// 记录一次重试。
    pub fn record_retry(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        inner.retries += 1;
    }

    /// 记录一次排队拒绝。
    pub fn record_queue_rejection(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        inner.queue_rejections += 1;
    }

    /// 生成快照。
    pub fn snapshot(&self) -> MetricsSnapshot {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut latencies: Vec<f64> = inner
            .samples
            .iter()
            .filter(|sample| sample.success)
            .map(|sample| sample.latency_ms)
            .collect();
        latencies.sort_by(f64::total_cmp);

        let mut by_model = Vec::with_capacity(inner.by_model.len());
        for (model, counters) in &inner.by_model {
            let mut model_latencies: Vec<f64> = inner
                .samples
                .iter()
                .filter(|sample| sample.model == *model && sample.success)
                .map(|sample| sample.latency_ms)
                .collect();
            model_latencies.sort_by(f64::total_cmp);
            let avg = if model_latencies.is_empty() {
                0.0
            } else {
                model_latencies.iter().sum::<f64>() / model_latencies.len() as f64
            };
            by_model.push(ModelMetric {
                model: model.clone(),
                requests: counters.requests,
                success: counters.success,
                failures: counters.failures,
                avg_latency_ms: round3(avg),
                p95_latency_ms: round3(percentile(&model_latencies, 0.95)),
                input_tokens: counters.input_tokens,
                output_tokens: counters.output_tokens,
                estimated_cost_usd: round6(counters.cost_usd),
            });
        }

        let avg = if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        };
        MetricsSnapshot {
            total_requests: inner.total_requests,
            success_requests: inner.success_requests,
            failed_requests: inner.failed_requests,
            retries: inner.retries,
            queue_rejections: inner.queue_rejections,
            avg_latency_ms: round3(avg),
            p50_latency_ms: round3(percentile(&latencies, 0.50)),
            p95_latency_ms: round3(percentile(&latencies, 0.95)),
            p99_latency_ms: round3(percentile(&latencies, 0.99)),
            input_tokens: inner.input_tokens,
            output_tokens: inner.output_tokens,
            estimated_cost_usd: round6(inner.cost_usd),
            by_model,
        }
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 * quantile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_computes_percentiles() {
        let metrics = ModelMetrics::new(100);
        for value in 1..=100 {
            metrics.record(
                "m",
                f64::from(value),
                true,
                Usage {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                    total_tokens: 3,
                },
                0.001,
            );
        }
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_requests, 100);
        assert_eq!(snapshot.success_requests, 100);
        assert_eq!(snapshot.p50_latency_ms, 50.0);
        assert_eq!(snapshot.p95_latency_ms, 95.0);
        assert_eq!(snapshot.p99_latency_ms, 99.0);
        assert_eq!(snapshot.input_tokens, 100);
        assert_eq!(snapshot.output_tokens, 200);
        assert!((snapshot.estimated_cost_usd - 0.1).abs() < 1e-6);
    }

    #[test]
    fn failures_are_counted_but_excluded_from_latency() {
        let metrics = ModelMetrics::default();
        metrics.record("m", 100.0, false, Usage::default(), 0.0);
        metrics.record("m", 10.0, true, Usage::default(), 0.0);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.failed_requests, 1);
        assert_eq!(snapshot.p95_latency_ms, 10.0);
        assert_eq!(snapshot.by_model[0].failures, 1);
    }
}
