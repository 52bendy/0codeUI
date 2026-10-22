//! 应用共享状态（注入 handler 与中间件）。

use std::sync::Arc;
use std::time::Instant;

use crate::config::AppConfig;
use crate::db::Db;
use crate::middleware::rate_limit::RateLimiter;
use crate::middleware::replay::{PendingReplayGuard, ReplayGuard};
use crate::middleware::telemetry_quota::TelemetryQuota;

/// 全局共享状态。
#[derive(Clone)]
pub struct AppState {
    /// 服务配置（只读）。
    pub config: Arc<AppConfig>,
    /// 基础限流器。
    pub rate_limiter: Arc<RateLimiter>,
    /// 埋点上报防刷配额（按用户维度）。
    pub telemetry_quota: Arc<TelemetryQuota>,
    /// 防重放校验器（REQ-202610-1186 接入点，当前为预留实现）。
    pub replay_guard: Arc<dyn ReplayGuard>,
    /// 数据库句柄。
    pub db: Arc<Db>,
    /// 进程启动时间（用于健康检查 uptime）。
    pub started_at: Instant,
}

impl AppState {
    /// 基于配置与数据库句柄构造共享状态。
    pub fn new(config: AppConfig, db: Db) -> Self {
        let rate_limiter = RateLimiter::new(
            config.rate_limit_rps,
            config.rate_limit_burst,
            config.rate_limit_enabled,
        );
        let telemetry_quota = TelemetryQuota::new(
            config.telemetry_quota_per_minute,
            config.telemetry_max_batch_size,
            config.telemetry_quota_enabled,
        );
        let replay_guard: Arc<dyn ReplayGuard> =
            Arc::new(PendingReplayGuard::new(config.telemetry_replay_enabled));
        Self {
            config: Arc::new(config),
            rate_limiter: Arc::new(rate_limiter),
            telemetry_quota: Arc::new(telemetry_quota),
            replay_guard,
            db: Arc::new(db),
            started_at: Instant::now(),
        }
    }

    /// 服务已运行秒数。
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}
