//! 结构化日志初始化（tracing）。

use tracing_subscriber::EnvFilter;

use crate::config::LogFormat;

/// 初始化全局 tracing subscriber。
///
/// - `log_level`：默认过滤级别（如 `info`、`debug`），可被环境变量 `RUST_LOG` 覆盖；
/// - `format`：`pretty`（本地开发）/ `json`（生产，便于日志采集）。
pub fn init_telemetry(log_level: &str, format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    match format {
        LogFormat::Json => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .flatten_event(true)
                .try_init();
        }
        LogFormat::Pretty => {
            let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
        }
    }
}
