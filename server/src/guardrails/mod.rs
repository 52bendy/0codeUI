//! 安全风控 Guardrails 与二次确认流（REQ-202610-1179）。
//!
//! 模块职责：
//! - [`policy`]：按能力风险等级判定是否需要二次确认；
//! - [`token`]：一次性确认令牌的生成、摘要与常量时间校验；
//! - [`model`]：确认单、审计事件与 HTTP 请求 / 响应模型；
//! - [`service`]：确认单创建、确认执行、拒绝取消、过期清理。

pub mod model;
pub mod policy;
pub mod service;
pub mod token;

pub use model::{
    ActionExecutionResult, ConfirmRequest, ConfirmationDecision, ConfirmationEvent,
    ConfirmationEventType, ConfirmationPayload, ConfirmationRecord, ConfirmationStatus,
    NewConfirmation, RejectRequest, DEFAULT_CONFIRMATION_TTL_SECS,
};
pub use policy::{assess_capabilities, assess_capability, assess_changes, RiskAssessment};
pub use service::ConfirmationService;
