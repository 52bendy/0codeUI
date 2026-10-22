//! 二次确认编排服务（REQ-202610-1179）。
//!
//! 安全属性：
//! - 写 / 删类动作只落确认单，绝不隐式执行；
//! - 令牌只存摘要，确认时校验用户归属 + 令牌 + 会话 + 过期时间；
//! - `PENDING -> EXECUTING` 原子抢占，保证「确认后仅执行一次」；
//! - 执行失败进入 `FAILED` 终态，不自动重试，避免重复副作用；
//! - 创建 / 确认 / 拒绝 / 过期 / 执行 / 失败均写审计事件。

use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, ErrorCode};
use crate::intent::capabilities;
use crate::intent::model::{ProposedChanges, RiskLevel};
use crate::models::{CreateOrderInput, CreateOrderItemInput};
use crate::repository::Repository;

use super::model::{
    risk_level_text, ActionExecutionResult, ConfirmRequest, ConfirmationDecision,
    ConfirmationEventType, ConfirmationPayload, ConfirmationRecord, ConfirmationStatus,
    NewConfirmation, RejectRequest,
};
use super::{policy, token};

/// 二次确认服务。
#[derive(Clone)]
pub struct ConfirmationService {
    repository: Repository,
    ttl: Duration,
}

impl ConfirmationService {
    /// 构造服务。
    pub fn new(repository: Repository, ttl_secs: u64) -> Self {
        Self {
            repository,
            ttl: Duration::from_secs(ttl_secs.max(1)),
        }
    }

    /// 用自定义 TTL 构造（测试 / 特殊部署）。
    pub fn with_ttl(repository: Repository, ttl: Duration) -> Self {
        Self { repository, ttl }
    }

    /// 创建确认单并返回确认卡片载荷。
    pub async fn create(
        &self,
        user_id: &str,
        session_id: &str,
        intent_summary: &str,
        proposed_changes: &ProposedChanges,
        confirmation_prompt: &str,
    ) -> Result<ConfirmationPayload, AppError> {
        validate_user_id(user_id)?;
        validate_token("session_id", session_id, 128)?;
        validate_text("intent_summary", intent_summary, 500)?;
        validate_text("confirmation_prompt", confirmation_prompt, 1000)?;

        let assessment = policy::assess_changes(proposed_changes)?;
        if !assessment.requires_confirmation {
            return Err(AppError::internal(
                "refusing to create a confirmation for a read-only action",
            ));
        }
        for (capability, parameters) in policy::actions_of(proposed_changes) {
            capabilities::validate_parameters(capability, parameters)
                .map_err(AppError::invalid_param)?;
        }

        let now = Utc::now();
        let expires_at = now
            + chrono::Duration::from_std(self.ttl)
                .map_err(|error| AppError::internal(error.to_string()))?;
        let confirmation_id = format!("CNF_{}", uuid::Uuid::new_v4().simple());
        let confirmation_token = token::generate();
        let token_hash = token::hash(&confirmation_token);
        let proposed_json = serde_json::to_string(proposed_changes)
            .map_err(|error| AppError::internal(error.to_string()))?;

        self.repository
            .create_confirmation(&NewConfirmation {
                id: confirmation_id.clone(),
                user_id: user_id.to_string(),
                session_id: session_id.to_string(),
                intent_summary: intent_summary.to_string(),
                risk_level: assessment.level,
                proposed_changes: proposed_json,
                token_hash,
                created_at: now,
                expires_at,
            })
            .await?;

        Ok(ConfirmationPayload {
            confirmation_id,
            confirmation_token,
            risk_level: assessment.level,
            intent_summary: intent_summary.to_string(),
            proposed_changes: proposed_changes.clone(),
            confirmation_prompt: confirmation_prompt.to_string(),
            expires_at,
            expires_in_secs: self.ttl.as_secs(),
        })
    }

    /// 确认并执行（仅执行一次）。
    pub async fn confirm(
        &self,
        actor_user_id: &str,
        request: ConfirmRequest,
    ) -> Result<ConfirmationDecision, AppError> {
        validate_user_id(actor_user_id)?;
        validate_confirmation_id(&request.confirmation_id)?;
        validate_token_format(&request.confirmation_token)?;
        if let Some(session_id) = &request.session_id {
            validate_token("session_id", session_id, 128)?;
        }

        let record = self.load_for_actor(actor_user_id, &request).await?;
        self.ensure_pending(&record).await?;

        let now = Utc::now();
        if !self.repository.claim_confirmation(&record.id, now).await? {
            return Err(self.conflict_after_claim(&record.id).await?);
        }

        self.repository
            .append_confirmation_event(
                &record.id,
                ConfirmationEventType::Confirmed,
                Some(actor_user_id),
                Some(&format!("risk={}", risk_level_text(record_risk(&record)))),
                now,
            )
            .await?;

        let changes: ProposedChanges = record.proposed_changes().map_err(|error| {
            AppError::internal(format!("stored proposed_changes is invalid: {error}"))
        })?;

        match self.execute_actions(actor_user_id, &changes).await {
            Ok(results) => {
                let result_payload = serde_json::to_string(&results)
                    .map_err(|error| AppError::internal(error.to_string()))?;
                let executed_at = Utc::now();
                self.repository
                    .finish_confirmation(
                        &record.id,
                        ConfirmationStatus::Executed,
                        Some(&result_payload),
                        None,
                        executed_at,
                    )
                    .await?;
                self.repository
                    .append_confirmation_event(
                        &record.id,
                        ConfirmationEventType::Executed,
                        Some(actor_user_id),
                        Some(&format!("{} action(s) executed", results.len())),
                        executed_at,
                    )
                    .await?;
                Ok(ConfirmationDecision {
                    confirmation_id: record.id,
                    status: ConfirmationStatus::Executed,
                    decided_at: now,
                    executed_at: Some(executed_at),
                    results,
                    reason: None,
                })
            }
            Err(error) => {
                let failed_at = Utc::now();
                let detail = error
                    .detail
                    .clone()
                    .unwrap_or_else(|| error.message.clone());
                let _ = self
                    .repository
                    .finish_confirmation(
                        &record.id,
                        ConfirmationStatus::Failed,
                        None,
                        Some(&detail),
                        failed_at,
                    )
                    .await;
                let _ = self
                    .repository
                    .append_confirmation_event(
                        &record.id,
                        ConfirmationEventType::Failed,
                        Some(actor_user_id),
                        Some(&detail),
                        failed_at,
                    )
                    .await;
                Err(execution_failed(detail))
            }
        }
    }

    /// 拒绝 / 取消确认。
    pub async fn reject(
        &self,
        actor_user_id: &str,
        request: RejectRequest,
    ) -> Result<ConfirmationDecision, AppError> {
        validate_user_id(actor_user_id)?;
        validate_confirmation_id(&request.confirmation_id)?;
        validate_token_format(&request.confirmation_token)?;
        if let Some(reason) = &request.reason {
            validate_text("reason", reason, 500)?;
        }

        let record = self
            .load_for_actor(
                actor_user_id,
                &ConfirmRequest {
                    confirmation_id: request.confirmation_id.clone(),
                    confirmation_token: request.confirmation_token.clone(),
                    session_id: None,
                },
            )
            .await?;
        self.ensure_pending(&record).await?;

        let now = Utc::now();
        if !self
            .repository
            .reject_confirmation(&record.id, now, request.reason.as_deref())
            .await?
        {
            return Err(self.conflict_after_claim(&record.id).await?);
        }
        self.repository
            .append_confirmation_event(
                &record.id,
                ConfirmationEventType::Rejected,
                Some(actor_user_id),
                request.reason.as_deref(),
                now,
            )
            .await?;

        Ok(ConfirmationDecision {
            confirmation_id: record.id,
            status: ConfirmationStatus::Rejected,
            decided_at: now,
            executed_at: None,
            results: Vec::new(),
            reason: request.reason,
        })
    }

    /// 批量清理过期确认单（供后台定时任务调用）。
    pub async fn expire_stale(&self, limit: usize) -> Result<usize, AppError> {
        Ok(self
            .repository
            .expire_stale_confirmations(Utc::now(), limit)
            .await?)
    }

    async fn load_for_actor(
        &self,
        actor_user_id: &str,
        request: &ConfirmRequest,
    ) -> Result<ConfirmationRecord, AppError> {
        let record = self
            .repository
            .find_confirmation(&request.confirmation_id)
            .await?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "confirmation not found"))?;

        if record.user_id != actor_user_id {
            return Err(AppError::new(
                ErrorCode::Forbidden,
                "confirmation does not belong to the current user",
            ));
        }
        if !token::verify(&request.confirmation_token, &record.token_hash) {
            return Err(AppError::new(
                ErrorCode::InvalidParam,
                "invalid confirmation token",
            ));
        }
        if let Some(session_id) = &request.session_id {
            if session_id != &record.session_id {
                return Err(AppError::new(
                    ErrorCode::InvalidParam,
                    "confirmation does not belong to this session",
                ));
            }
        }
        Ok(record)
    }

    /// 检查是否仍可决策；过期时落 `EXPIRED` 终态并返回 409。
    async fn ensure_pending(&self, record: &ConfirmationRecord) -> Result<(), AppError> {
        if record.status != ConfirmationStatus::Pending {
            return Err(AppError::new(
                ErrorCode::Conflict,
                format!(
                    "confirmation is already {}",
                    record.status.as_str().to_ascii_lowercase()
                ),
            ));
        }
        let now = Utc::now();
        if record.expires_at <= now {
            if self.repository.expire_confirmation(&record.id, now).await? {
                self.repository
                    .append_confirmation_event(
                        &record.id,
                        ConfirmationEventType::Expired,
                        None,
                        Some("confirmation expired before user decision"),
                        now,
                    )
                    .await?;
            }
            return Err(AppError::new(ErrorCode::Conflict, "confirmation expired"));
        }
        Ok(())
    }

    async fn conflict_after_claim(&self, confirmation_id: &str) -> Result<AppError, AppError> {
        let latest = self
            .repository
            .find_confirmation(confirmation_id)
            .await?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "confirmation not found"))?;
        Ok(AppError::new(
            ErrorCode::Conflict,
            format!(
                "confirmation is already {}",
                latest.status.as_str().to_ascii_lowercase()
            ),
        ))
    }

    async fn execute_actions(
        &self,
        actor_user_id: &str,
        changes: &ProposedChanges,
    ) -> Result<Vec<ActionExecutionResult>, AppError> {
        let mut results = Vec::new();
        for (capability, parameters) in policy::actions_of(changes) {
            capabilities::validate_parameters(capability, parameters)
                .map_err(AppError::invalid_param)?;
            let data = self
                .execute_one(actor_user_id, capability, parameters)
                .await?;
            results.push(ActionExecutionResult {
                target_capability: capability.to_string(),
                status: "SUCCESS".to_string(),
                data,
            });
        }
        Ok(results)
    }

    async fn execute_one(
        &self,
        actor_user_id: &str,
        capability: &str,
        parameters: &Value,
    ) -> Result<Value, AppError> {
        match capability {
            capabilities::UPDATE_ORDER_ADDRESS => {
                let order_id = required_str(parameters, "order_id")?;
                let new_address = required_str(parameters, "new_address")?;
                self.ensure_order_owned(actor_user_id, order_id).await?;
                let order = self
                    .repository
                    .update_order_address(Some(actor_user_id), order_id, new_address)
                    .await?;
                to_value(order)
            }
            capabilities::UPDATE_ITEM_STATUS => {
                let item_id = required_str(parameters, "item_id")?;
                let status = required_str(parameters, "status")?;
                let item = self.repository.find_order_item(item_id).await?;
                self.ensure_order_owned(actor_user_id, &item.order_id)
                    .await?;
                let updated = self
                    .repository
                    .update_item_status(Some(actor_user_id), item_id, status)
                    .await?;
                to_value(updated)
            }
            capabilities::DELETE_RECORD => {
                let entity_type = required_str(parameters, "entity_type")?;
                let entity_id = required_str(parameters, "entity_id")?;
                self.ensure_deletable(actor_user_id, entity_type, entity_id)
                    .await?;
                let deleted = self
                    .repository
                    .delete_record(Some(actor_user_id), entity_type, entity_id)
                    .await?;
                to_value(deleted)
            }
            capabilities::CREATE_ORDER => {
                let input = parse_create_order(actor_user_id, parameters)?;
                let order = self.repository.create_order(&input).await?;
                to_value(order)
            }
            capabilities::QUERY_USER_BALANCE => {
                let balance = self.repository.query_user_balance(actor_user_id).await?;
                to_value(balance)
            }
            capabilities::QUERY_ORDER_BUSINESS_DATA => {
                let order_id = required_str(parameters, "order_id")?;
                self.ensure_order_owned(actor_user_id, order_id).await?;
                let data = self.repository.order_business_data(order_id).await?;
                to_value(data)
            }
            other => Err(AppError::internal(format!(
                "capability `{other}` is not executable"
            ))),
        }
    }

    async fn ensure_order_owned(
        &self,
        actor_user_id: &str,
        order_id: &str,
    ) -> Result<(), AppError> {
        let order = self.repository.find_order(order_id).await?;
        if order.user_id == actor_user_id {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::Forbidden,
                "无权操作其他用户的订单",
            ))
        }
    }

    async fn ensure_deletable(
        &self,
        actor_user_id: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<(), AppError> {
        match entity_type {
            "order" => self.ensure_order_owned(actor_user_id, entity_id).await,
            "order_item" => {
                let item = self.repository.find_order_item(entity_id).await?;
                self.ensure_order_owned(actor_user_id, &item.order_id).await
            }
            "product" => Err(AppError::new(
                ErrorCode::Forbidden,
                "商品为全局目录，暂不支持通过意图接口删除",
            )),
            "operation_record" => Err(AppError::new(
                ErrorCode::Forbidden,
                "审计记录不允许通过意图接口删除",
            )),
            other => Err(AppError::invalid_param(format!(
                "unsupported entity type `{other}`"
            ))),
        }
    }
}

fn record_risk(record: &ConfirmationRecord) -> RiskLevel {
    match record.risk_level.as_str() {
        "DELETE" => RiskLevel::Delete,
        "WRITE" => RiskLevel::Write,
        _ => RiskLevel::Read,
    }
}

fn required_str<'a>(parameters: &'a Value, key: &str) -> Result<&'a str, AppError> {
    parameters
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::invalid_param(format!("{key} must be a non-empty string")))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateOrderParameters {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    shipping_address: Option<String>,
    items: Vec<CreateOrderItemInput>,
}

fn parse_create_order(
    actor_user_id: &str,
    parameters: &Value,
) -> Result<CreateOrderInput, AppError> {
    let parsed: CreateOrderParameters =
        serde_json::from_value(parameters.clone()).map_err(|error| {
            AppError::invalid_param(format!("invalid create_order parameters: {error}"))
        })?;
    if let Some(requested) = parsed.user_id.as_deref() {
        if requested != actor_user_id {
            return Err(AppError::new(
                ErrorCode::Forbidden,
                "user_id 与当前登录用户不一致",
            ));
        }
    }
    Ok(CreateOrderInput {
        user_id: actor_user_id.to_string(),
        order_id: parsed.order_id,
        shipping_address: parsed.shipping_address,
        items: parsed.items,
    })
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(|error| AppError::internal(error.to_string()))
}

fn execution_failed(detail: impl std::fmt::Display) -> AppError {
    let mut error = AppError::new(ErrorCode::Internal, "confirmation execution failed");
    error.detail = Some(detail.to_string());
    error
}

fn validate_user_id(user_id: &str) -> Result<(), AppError> {
    validate_token("user_id", user_id, 64)
}

fn validate_confirmation_id(confirmation_id: &str) -> Result<(), AppError> {
    let Some(rest) = confirmation_id.strip_prefix("CNF_") else {
        return Err(AppError::invalid_param("invalid confirmation_id"));
    };
    if rest.is_empty() || rest.len() > 60 || !rest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::invalid_param("invalid confirmation_id"));
    }
    Ok(())
}

fn validate_token_format(token_value: &str) -> Result<(), AppError> {
    if token::is_well_formed(token_value) {
        Ok(())
    } else {
        Err(AppError::invalid_param("invalid confirmation_token"))
    }
}

fn validate_text(field: &str, value: &str, max_len: usize) -> Result<(), AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::invalid_param(format!(
            "{field} must not be empty"
        )));
    }
    if value.chars().count() > max_len {
        return Err(AppError::invalid_param(format!(
            "{field} must be at most {max_len} characters"
        )));
    }
    Ok(())
}

fn validate_token(field: &str, value: &str, max_len: usize) -> Result<(), AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_len {
        return Err(AppError::invalid_param(format!(
            "{field} must be 1..={max_len} characters"
        )));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "._:-".contains(ch))
    {
        return Err(AppError::invalid_param(format!(
            "{field} may only contain ASCII letters, digits and `._:-`"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_id_validation_accepts_generated_shape() {
        assert!(validate_confirmation_id("CNF_0123456789abcdef0123456789abcdef").is_ok());
        assert!(validate_confirmation_id("CNF_").is_err());
        assert!(validate_confirmation_id("CNF_xyz").is_err());
        assert!(validate_confirmation_id("OTHER_123").is_err());
    }

    #[test]
    fn token_format_is_enforced() {
        assert!(validate_token_format(&token::generate()).is_ok());
        assert!(validate_token_format("cct_1234").is_err());
    }

    #[test]
    fn create_order_forces_actor_identity() {
        let input = parse_create_order(
            "usr_9527",
            &serde_json::json!({
                "items": [{"product_id": "PRD_1", "qty": 1}]
            }),
        )
        .unwrap();
        assert_eq!(input.user_id, "usr_9527");
        assert!(parse_create_order(
            "usr_9527",
            &serde_json::json!({
                "user_id": "usr_0001",
                "items": [{"product_id": "PRD_1", "qty": 1}]
            })
        )
        .is_err());
    }
}
