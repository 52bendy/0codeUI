//! 应用共享状态（注入 handler 与中间件）。

use std::sync::Arc;
use std::time::Instant;

use crate::config::AppConfig;
use crate::db::Db;
use crate::middleware::rate_limit::RateLimiter;
use crate::obfuscation::ObfuscationEngine;

/// 全局共享状态。
#[derive(Clone)]
pub struct AppState {
    /// 服务配置（只读）。
    pub config: Arc<AppConfig>,
    /// 基础限流器。
    pub rate_limiter: Arc<RateLimiter>,
    /// 数据库句柄。
    pub db: Arc<Db>,
    /// 输出混淆引擎（会话级类名映射表）。
    pub obfuscation: Arc<ObfuscationEngine>,
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
        let obfuscation = Arc::new(ObfuscationEngine::new(
            config.ui_obfuscation_session_ttl_secs,
            config.ui_obfuscation_max_sessions,
        ));
        Self {
            config: Arc::new(config),
            rate_limiter: Arc::new(rate_limiter),
            db: Arc::new(db),
            obfuscation,
            started_at: Instant::now(),
        }
    }

    /// 服务已运行秒数。
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}
