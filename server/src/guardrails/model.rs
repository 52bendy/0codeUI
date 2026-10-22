//! 二次确认的领域模型（REQ-202610-1179）。
//!
//! 写 / 删类意图不会直接执行：`/api/v1/intent/execute` 返回
//! [`ConfirmationPayload`]（含一次性令牌），客户端展示确认卡片后调用
//! `/api/v1/intent/confirm` 或 `/api/v1/intent/reject`。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::intent::model::{ProposedChanges, RiskLevel};

/// 确认令牌有效期默认值（秒）。
pub const DEFAULT_CONFIRMATION_TTL_SECS: u64 = 300;

/// 风险等级的数据库 / JSON 文本。
pub const fn risk_level_text(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Read => "READ",
        RiskLevel::Write => "WRITE",
        RiskLevel::Delete => "DELETE",
    }
}

/// 确认单状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfirmationStatus {
    /// 等待用户确认 / 拒绝。
    Pending,
    /// 已确认，等待执行（保留状态，便于审计）。
    Confirmed,
    /// 用户已拒绝。
    Rejected,
    /// 已过期，未执行。
    Expired,
    /// 执行中（原子占位，防止并发重复执行）。
    Executing,
    /// 已执行成功。
    Executed,
    /// 执行失败（不自动重试，避免重复副作用）。
    Failed,
}

impl ConfirmationStatus {
    /// 数据库文本。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Confirmed => "CONFIRMED",
            Self::Rejected => "REJECTED",
            Self::Expired => "EXPIRED",
            Self::Executing => "EXECUTING",
            Self::Executed => "EXECUTED",
            Self::Failed => "FAILED",
        }
    }

    /// 从数据库文本还原。
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "PENDING" => Some(Self::Pending),
            "CONFIRMED" => Some(Self::Confirmed),
            "REJECTED" => Some(Self::Rejected),
            "EXPIRED" => Some(Self::Expired),
            "EXECUTING" => Some(Self::Executing),
            "EXECUTED" => Some(Self::Executed),
            "FAILED" => Some(Self::Failed),
            _ => None,
        }
    }

    /// 是否已进入终态（不可再次确认 / 拒绝）。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Rejected | Self::Expired | Self::Executed | Self::Failed
        )
    }
}

/// 确认卡片载荷（`/api/v1/intent/execute` 的 `data.confirmation`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationPayload {
    /// 确认单编号（`CNF_<uuid>`）。
    pub confirmation_id: String,
    /// 一次性确认令牌；仅创建时返回，服务端只保存摘要。
    pub confirmation_token: String,
    /// 风险等级：`WRITE` / `DELETE`。
    pub risk_level: RiskLevel,
    /// 意图摘要。
    pub intent_summary: String,
    /// 待执行的动作快照。
    pub proposed_changes: ProposedChanges,
    /// 确认文案。
    pub confirmation_prompt: String,
    /// 过期时间（RFC3339）。
    pub expires_at: DateTime<Utc>,
    /// 距离过期的秒数。
    pub expires_in_secs: u64,
}

/// 确认请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmRequest {
    /// 确认单编号。
    pub confirmation_id: String,
    /// 一次性确认令牌。
    pub confirmation_token: String,
    /// 可选会话标识；提供时必须与创建时一致。
    pub session_id: Option<String>,
}

/// 拒绝 / 取消请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectRequest {
    /// 确认单编号。
    pub confirmation_id: String,
    /// 一次性确认令牌。
    pub confirmation_token: String,
    /// 可选拒绝原因（≤500 字符）。
    pub reason: Option<String>,
}

/// 单个动作的执行结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionExecutionResult {
    /// 目标原子能力。
    pub target_capability: String,
    /// 执行状态。
    pub status: String,
    /// 能力返回数据。
    pub data: Value,
}

/// 确认 / 拒绝后的响应数据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationDecision {
    /// 确认单编号。
    pub confirmation_id: String,
    /// 终态：`EXECUTED` / `REJECTED` / `FAILED`。
    pub status: ConfirmationStatus,
    /// 决策时间。
    pub decided_at: DateTime<Utc>,
    /// 执行完成时间（仅执行成功时）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executed_at: Option<DateTime<Utc>>,
    /// 各动作执行结果（仅执行成功时）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<ActionExecutionResult>,
    /// 拒绝原因（仅拒绝时）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 新建确认单的持久化输入。
#[derive(Debug, Clone)]
pub struct NewConfirmation {
    /// 确认单编号。
    pub id: String,
    /// 归属用户。
    pub user_id: String,
    /// 会话标识。
    pub session_id: String,
    /// 意图摘要。
    pub intent_summary: String,
    /// 风险等级。
    pub risk_level: RiskLevel,
    /// 动作快照 JSON。
    pub proposed_changes: String,
    /// 令牌摘要（SHA-256 hex）。
    pub token_hash: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 过期时间。
    pub expires_at: DateTime<Utc>,
}

/// 数据库中的确认单。
#[derive(Debug, Clone)]
pub struct ConfirmationRecord {
    /// 确认单编号。
    pub id: String,
    /// 归属用户。
    pub user_id: String,
    /// 会话标识。
    pub session_id: String,
    /// 意图摘要。
    pub intent_summary: String,
    /// 风险等级文本。
    pub risk_level: String,
    /// 动作快照 JSON。
    pub proposed_changes: String,
    /// 令牌摘要。
    pub token_hash: String,
    /// 状态。
    pub status: ConfirmationStatus,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 过期时间。
    pub expires_at: DateTime<Utc>,
    /// 决策时间。
    pub decided_at: Option<DateTime<Utc>>,
    /// 执行完成时间。
    pub executed_at: Option<DateTime<Utc>>,
    /// 执行结果 JSON。
    pub result_payload: Option<String>,
    /// 失败原因。
    pub failure_reason: Option<String>,
}

impl ConfirmationRecord {
    /// 是否已过期（仅对 PENDING 有意义）。
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.status == ConfirmationStatus::Pending && self.expires_at <= now
    }

    /// 解析动作快照。
    pub fn proposed_changes(&self) -> Result<ProposedChanges, serde_json::Error> {
        serde_json::from_str(&self.proposed_changes)
    }
}

/// 审计事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationEventType {
    /// 确认单创建。
    Created,
    /// 用户确认，开始执行。
    Confirmed,
    /// 用户拒绝。
    Rejected,
    /// 过期。
    Expired,
    /// 执行成功。
    Executed,
    /// 执行失败。
    Failed,
}

impl ConfirmationEventType {
    /// 数据库文本。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::Confirmed => "CONFIRMED",
            Self::Rejected => "REJECTED",
            Self::Expired => "EXPIRED",
            Self::Executed => "EXECUTED",
            Self::Failed => "FAILED",
        }
    }
}

/// 审计事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationEvent {
    /// 事件编号。
    pub id: String,
    /// 确认单编号。
    pub confirmation_id: String,
    /// 事件类型。
    pub event_type: String,
    /// 操作人。
    pub actor_user_id: Option<String>,
    /// 事件详情。
    pub detail: Option<String>,
    /// 发生时间。
    pub created_at: DateTime<Utc>,
}
