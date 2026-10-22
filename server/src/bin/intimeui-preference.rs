//! 偏好数据管道 CLI（REQ-202610-1188）。
//!
//! 用法：
//! ```bash
//! intimeui-preference stats
//! intimeui-preference sample --limit 20 --seed review-2026-10-22
//! intimeui-preference export --output data/preference_exports --limit 5000
//! ```
//!
//! `export` 默认只导出 ACCEPTED 记录；写盘成功后才落库导出批次并标记 EXPORTED。
//! `--dry-run` 仅生成文件、不改变数据库状态，适合演练与人工抽检。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::ExitCode;

use intimeui_server::config::Environment;
use intimeui_server::db::Db;
use intimeui_server::preference::config::PreferenceConfig;
use intimeui_server::preference::model::{ExportOptions, PairFilter, PreferenceSource};
use intimeui_server::preference::service::PreferenceService;
use intimeui_server::repository::Repository;

/// 命令行选项。
#[derive(Debug, Default)]
struct Options {
    values: HashMap<String, String>,
    flags: HashSet<String>,
}

impl Options {
    /// 读取必填字符串。
    fn require(&self, key: &str) -> Result<&str, String> {
        self.values
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("--{key} is required"))
    }

    /// 读取可选字符串。
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// 读取可选整数。
    fn get_usize(&self, key: &str) -> Result<Option<usize>, String> {
        self.values
            .get(key)
            .map(|value| {
                value
                    .parse::<usize>()
                    .map_err(|_| format!("--{key} must be a positive integer"))
            })
            .transpose()
    }

    /// 读取可选浮点数。
    fn get_f64(&self, key: &str) -> Result<Option<f64>, String> {
        self.values
            .get(key)
            .map(|value| {
                value
                    .parse::<f64>()
                    .map_err(|_| format!("--{key} must be a number"))
            })
            .transpose()
    }

    /// 读取来源过滤。
    fn get_source(&self) -> Result<Option<String>, String> {
        self.get("source")
            .map(|value| {
                PreferenceSource::parse(value)
                    .map(|source| source.as_str().to_string())
                    .ok_or_else(|| format!("unknown source `{value}`"))
            })
            .transpose()
    }

    /// 校验选项白名单。
    fn allow(&self, values: &[&str], flags: &[&str]) -> Result<(), String> {
        for key in self.values.keys() {
            if !values.contains(&key.as_str()) {
                return Err(format!("unknown option `--{key}`"));
            }
        }
        for key in self.flags.iter() {
            if !flags.contains(&key.as_str()) {
                return Err(format!("unknown option `--{key}`"));
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return ExitCode::FAILURE;
    };
    if matches!(command, "--help" | "-h" | "help") {
        print_help();
        return ExitCode::SUCCESS;
    }
    let options = match parse_options(&args[1..]) {
        Ok(options) => options,
        Err(message) => return fail(&message),
    };

    let result = match command {
        "stats" => run_stats(&options).await,
        "sample" => run_sample(&options).await,
        "export" => run_export(&options).await,
        other => Err(format!("unknown command `{other}` (try --help)")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

/// 打印帮助。
fn print_help() {
    println!(
        "Usage: intimeui-preference <stats|sample|export> [options]\n\
         \n\
         Global options:\n\
           --database-url <URL>       SQLite URL (default: DATABASE_URL or sqlite://data/intimeui.db)\n\
         \n\
         stats\n\
           Print pipeline statistics as JSON.\n\
         \n\
         sample [--limit N] [--seed S] [--scene-code C] [--source S]\n\
           Deterministic stratified sample for manual inspection (max 200).\n\
         \n\
         export --output <DIR> [--limit N] [--min-quality Q]\n\
                [--max-per-user N] [--max-per-scene N] [--scene-code C]\n\
                [--source S] [--dry-run]\n\
           Write dpo-<dataset_id>.jsonl + manifest to DIR."
    );
}

/// 解析 `--key value` / `--flag`。
fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0usize;
    while index < args.len() {
        let argument = &args[index];
        let name = argument
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument `{argument}`"))?;
        if name.is_empty() {
            return Err("empty option name".to_string());
        }
        let next_is_value = args
            .get(index + 1)
            .map(|value| !value.starts_with("--"))
            .unwrap_or(false);
        if next_is_value {
            options
                .values
                .insert(name.to_string(), args[index + 1].clone());
            index += 2;
        } else {
            options.flags.insert(name.to_string());
            index += 1;
        }
    }
    Ok(options)
}

/// 连接数据库并执行迁移。
async fn connect(options: &Options) -> Result<Db, String> {
    let database_url = options
        .get("database-url")
        .map(str::to_string)
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "sqlite://data/intimeui.db".to_string());
    let db = Db::connect(&database_url)
        .await
        .map_err(|error| format!("failed to connect database `{database_url}`: {error}"))?;
    db.migrate()
        .await
        .map_err(|error| format!("failed to run migrations: {error}"))?;
    Ok(db)
}

/// 加载偏好配置。
fn preference_config() -> Result<PreferenceConfig, String> {
    let environment = std::env::var("APP_ENV")
        .ok()
        .and_then(|value| value.parse::<Environment>().ok())
        .unwrap_or(Environment::Dev);
    PreferenceConfig::from_env(environment).map_err(|error| error.to_string())
}

/// `stats` 子命令。
async fn run_stats(options: &Options) -> Result<(), String> {
    options.allow(&["database-url"], &[])?;
    let db = connect(options).await?;
    let config = preference_config()?;
    let repository = Repository::from_db(&db);
    let service = PreferenceService::new(&repository, &config);
    let stats = service.stats().await.map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&stats).map_err(|error| error.to_string())?
    );
    Ok(())
}

/// `sample` 子命令。
async fn run_sample(options: &Options) -> Result<(), String> {
    options.allow(
        &["database-url", "limit", "seed", "scene-code", "source"],
        &[],
    )?;
    let limit = options.get_usize("limit")?.unwrap_or(20);
    let seed = options.get("seed").unwrap_or("default");
    let filter = PairFilter {
        scene_code: options.get("scene-code").map(str::to_string),
        source: options.get_source()?,
        ..PairFilter::default()
    };
    let db = connect(options).await?;
    let config = preference_config()?;
    let repository = Repository::from_db(&db);
    let service = PreferenceService::new(&repository, &config);
    let sample = service
        .sample(&filter, limit, seed)
        .await
        .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&sample).map_err(|error| error.to_string())?
    );
    Ok(())
}

/// `export` 子命令。
async fn run_export(options: &Options) -> Result<(), String> {
    options.allow(
        &[
            "database-url",
            "output",
            "limit",
            "min-quality",
            "max-per-user",
            "max-per-scene",
            "scene-code",
            "source",
        ],
        &["dry-run"],
    )?;
    let output = options.require("output")?.to_string();
    let db = connect(options).await?;
    let config = preference_config()?;
    let repository = Repository::from_db(&db);
    let service = PreferenceService::new(&repository, &config);
    let export_options = ExportOptions {
        limit: options
            .get_usize("limit")?
            .unwrap_or(config.export_max_records),
        min_quality: options
            .get_f64("min-quality")?
            .unwrap_or(config.min_quality),
        max_per_user: options
            .get_usize("max-per-user")?
            .unwrap_or(config.export_max_per_user),
        max_per_scene: options
            .get_usize("max-per-scene")?
            .unwrap_or(config.export_max_per_scene),
        scene_code: options.get("scene-code").map(str::to_string),
        source: options.get_source()?,
        include_pending: false,
        dry_run: true,
    };
    let bundle = service
        .export(&export_options)
        .await
        .map_err(|error| error.to_string())?;

    std::fs::create_dir_all(&output)
        .map_err(|error| format!("failed to create output directory `{output}`: {error}"))?;
    let jsonl_path = Path::new(&output).join(format!("dpo-{}.jsonl", bundle.manifest.dataset_id));
    let manifest_path =
        Path::new(&output).join(format!("dpo-{}.manifest.json", bundle.manifest.dataset_id));
    let jsonl = bundle.to_jsonl().map_err(|error| error.to_string())?;
    std::fs::write(&jsonl_path, jsonl)
        .map_err(|error| format!("failed to write `{}`: {error}", jsonl_path.display()))?;
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&bundle.manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("failed to write `{}`: {error}", manifest_path.display()))?;

    if !options.flags.contains("dry-run") {
        service
            .commit_export(&bundle)
            .await
            .map_err(|error| error.to_string())?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&bundle.manifest).map_err(|error| error.to_string())?
    );
    eprintln!(
        "dataset written: {} records -> {} (manifest: {})",
        bundle.manifest.record_count,
        jsonl_path.display(),
        manifest_path.display()
    );
    Ok(())
}

/// 打印错误并返回失败退出码。
fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}
