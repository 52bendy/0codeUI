//! 应用共享状态（注入 handler 与中间件）。

use std::sync::Arc;
use std::time::Instant;

use crate::config::AppConfig;
use crate::db::Db;
use crate::intent::planner::{IntentPlanner, RuleBasedIntentPlanner};
use crate::intent::service::IntentService;
use crate::intent_agent::ToolCallAgent;
use crate::middleware::rate_limit::RateLimiter;
use crate::repository::Repository;

/// 全局共享状态。
#[derive(Clone)]
pub struct AppState {
    /// 服务配置（只读）。
    pub config: Arc<AppConfig>,
    /// 基础限流器。
    pub rate_limiter: Arc<RateLimiter>,
    /// 数据库句柄。
    pub db: Arc<Db>,
    /// 意图理解与 Tool Call Agent。
    pub intent_agent: Arc<ToolCallAgent>,
    /// 意图执行编排服务。
    pub intent_service: Arc<IntentService>,
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
        let intent_agent = Arc::new(ToolCallAgent::rule_based(config.intent_agent_max_attempts));
        let intent_service = IntentService::new(
            Repository::from_db(&db),
            Arc::new(RuleBasedIntentPlanner),
            config.intent_session_ttl_secs,
        );
        Self {
            config: Arc::new(config),
            rate_limiter: Arc::new(rate_limiter),
            db: Arc::new(db),
            intent_agent,
            intent_service: Arc::new(intent_service),
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
