//! 应用共享状态（注入 handler 与中间件）。

use std::sync::Arc;
use std::time::Instant;

use crate::config::AppConfig;
use crate::db::Db;
use crate::guardrails::ConfirmationService;
use crate::intent::planner::{IntentPlanner, RuleBasedIntentPlanner};
use crate::intent::service::IntentService;
use crate::intent_agent::ToolCallAgent;
use crate::middleware::rate_limit::RateLimiter;
use crate::middleware::telemetry_quota::TelemetryQuota;
use crate::replay::ReplayService;
use crate::repository::Repository;

/// 全局共享状态。
#[derive(Clone)]
pub struct AppState {
    /// 服务配置（只读）。
    pub config: Arc<AppConfig>,
    /// 基础限流器。
    pub rate_limiter: Arc<RateLimiter>,
    /// 埋点上报防刷配额（按用户维度）。
    pub telemetry_quota: Arc<TelemetryQuota>,
    /// API 防重放签名校验服务（REQ-202610-1186）。
    pub replay: Arc<ReplayService>,
    /// 数据库句柄。
    pub db: Arc<Db>,
    /// 意图理解与 Tool Call Agent。
    pub intent_agent: Arc<ToolCallAgent>,
    /// 意图执行编排服务。
    pub intent_service: Arc<IntentService>,
    /// 二次确认服务。
    pub confirmation_service: Arc<ConfirmationService>,
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
        let replay = Arc::new(ReplayService::new(&config, db.pool().clone()));
        let intent_agent = Arc::new(ToolCallAgent::rule_based(config.intent_agent_max_attempts));
        let repository = Repository::from_db(&db);
        let intent_service = IntentService::new(
            repository.clone(),
            Arc::new(RuleBasedIntentPlanner),
            config.intent_session_ttl_secs,
        );
        let confirmation_service =
            ConfirmationService::new(repository, config.intent_confirmation_ttl_secs);
        Self {
            config: Arc::new(config),
            rate_limiter: Arc::new(rate_limiter),
            telemetry_quota: Arc::new(telemetry_quota),
            replay,
            db: Arc::new(db),
            intent_agent,
            intent_service: Arc::new(intent_service),
            confirmation_service: Arc::new(confirmation_service),
            started_at: Instant::now(),
        }
    }

    /// 替换意图 Agent（用于测试或接入真实模型）。
    pub fn with_intent_agent(mut self, intent_agent: Arc<ToolCallAgent>) -> Self {
        self.intent_agent = intent_agent;
        self
    }

    /// 替换 #09 意图执行规划器（用于联调 Tool Call Agent 适配器）。
    pub fn with_intent_planner(mut self, planner: Arc<dyn IntentPlanner>) -> Self {
        self.intent_service = Arc::new(IntentService::new(
            Repository::from_db(&self.db),
            planner,
            self.config.intent_session_ttl_secs,
        ));
        self
    }

    /// 服务已运行秒数。
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}
