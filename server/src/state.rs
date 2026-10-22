//! 应用共享状态（注入 handler 与中间件）。

use std::sync::Arc;
use std::time::Instant;

use crate::config::AppConfig;
use crate::db::Db;
use crate::middleware::rate_limit::RateLimiter;
use crate::model::{ModelError, ModelGateway};
use crate::pipeline::Pipeline;

/// 全局共享状态。
#[derive(Clone)]
pub struct AppState {
    /// 服务配置（只读）。
    pub config: Arc<AppConfig>,
    /// 基础限流器。
    pub rate_limiter: Arc<RateLimiter>,
    /// 数据库句柄。
    pub db: Arc<Db>,
    /// 模型网关。
    pub model: Arc<ModelGateway>,
    /// AI 生成管线（编排 + 降级 + 缓存 + 指标）。
    pub pipeline: Arc<Pipeline>,
    /// 进程启动时间（用于健康检查 uptime）。
    pub started_at: Instant,
}

impl AppState {
    /// 基于配置与数据库句柄构造共享状态；模型提供者配置非法时返回错误。
    pub fn try_new(config: AppConfig, db: Db) -> Result<Self, ModelError> {
        let gateway = Arc::new(ModelGateway::from_config(&config.model)?);
        Ok(Self::with_gateway(config, db, gateway))
    }

    /// 基于配置与数据库句柄构造共享状态（测试 / 已校验配置）。
    pub fn new(config: AppConfig, db: Db) -> Self {
        Self::try_new(config, db).expect("invalid model provider configuration")
    }

    /// 注入自定义模型网关（测试 / 联调）。
    pub fn with_gateway(config: AppConfig, db: Db, gateway: Arc<ModelGateway>) -> Self {
        let rate_limiter = RateLimiter::new(
            config.rate_limit_rps,
            config.rate_limit_burst,
            config.rate_limit_enabled,
        );
        let db = Arc::new(db);
        let pipeline = Arc::new(Pipeline::new(
            config.pipeline.clone(),
            gateway.clone(),
            db.clone(),
        ));
        Self {
            config: Arc::new(config),
            rate_limiter: Arc::new(rate_limiter),
            db,
            model: gateway,
            pipeline,
            started_at: Instant::now(),
        }
    }

    /// 服务已运行秒数。
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}
