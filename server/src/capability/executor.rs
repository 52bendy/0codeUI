//! 原子能力执行器：权限校验 → 参数校验 → 执行 → 审计。
//!
//! 调度核心只依赖 [`CapabilityExecutor`] trait 与声明式 Schema；
//! 新增能力时实现 trait 并注册即可，不需要修改本文件。

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{AppError, ErrorCode};
use crate::repository::{Repository, RepositoryError};

use super::schema;
use super::spec::RiskLevel;
use super::{find, CapabilityEntry};

/// 首批能力默认授予的权限（当前鉴权模型为「已认证用户即本人数据操作者」）。
const DEFAULT_PERMISSIONS: &[&str] = &[
    "order:create",
    "balance:read",
    "order_item:update",
    "record:delete",
];

/// 删除商品 / 审计记录等全局数据所需的提升权限。
pub(crate) const ELEVATED_DELETE_PERMISSION: &str = "record:delete:any";

/// 能力执行上下文。
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// 操作人（鉴权用户）。
    pub actor_user_id: String,
    granted_permissions: BTreeSet<String>,
}

impl ExecutionContext {
    /// 已认证用户的默认上下文。
    ///
    /// `dev_mode` 为真时额外授予全局删除权限，便于本地联调；
    /// 生产环境需由后续 RBAC 能力显式授予。
    pub fn authenticated(actor_user_id: impl Into<String>, dev_mode: bool) -> Self {
        let mut permissions: BTreeSet<String> = DEFAULT_PERMISSIONS
            .iter()
            .map(|permission| (*permission).to_string())
            .collect();
        if dev_mode {
            permissions.insert(ELEVATED_DELETE_PERMISSION.to_string());
        }
        Self {
            actor_user_id: actor_user_id.into(),
            granted_permissions: permissions,
        }
    }

    /// 指定权限集合的上下文（测试 / 内部编排使用）。
    pub fn with_permissions(
        actor_user_id: impl Into<String>,
        permissions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            actor_user_id: actor_user_id.into(),
            granted_permissions: permissions
                .into_iter()
                .map(Into::into)
                .collect::<BTreeSet<_>>(),
        }
    }

    /// 是否拥有某个权限。
    pub fn has_permission(&self, permission: &str) -> bool {
        self.granted_permissions.contains(permission)
    }

    /// 是否拥有全部所需权限。
    fn has_permissions(&self, permissions: &[&str]) -> bool {
        permissions
            .iter()
            .all(|permission| self.has_permission(permission))
    }
}

/// 能力执行错误。
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    /// 能力未注册。
    #[error("unknown capability `{0}`")]
    UnknownCapability(String),
    /// 参数不符合声明式 Schema。
    #[error("{0}")]
    InvalidParameters(String),
    /// 权限不足或越权访问。
    #[error("{0}")]
    Forbidden(String),
    /// 仓储层错误。
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    /// 内部错误（序列化等）。
    #[error("{0}")]
    Internal(String),
}

impl From<CapabilityError> for AppError {
    fn from(error: CapabilityError) -> Self {
        match error {
            CapabilityError::UnknownCapability(name) => {
                AppError::new(ErrorCode::NotFound, format!("unknown capability: {name}"))
            }
            CapabilityError::InvalidParameters(message) => AppError::invalid_param(message),
            CapabilityError::Forbidden(message) => AppError::new(ErrorCode::Forbidden, message),
            CapabilityError::Repository(error) => error.into(),
            CapabilityError::Internal(message) => AppError::internal(message),
        }
    }
}

/// 能力执行结果。
#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    /// 能力名称。
    pub capability: &'static str,
    /// 风险等级。
    pub risk_level: RiskLevel,
    /// 能力返回值（JSON）。
    pub result: Value,
}

/// 单个能力的执行器。
#[async_trait]
pub(crate) trait CapabilityExecutor: Send + Sync {
    /// 执行能力（参数已通过 Schema 校验）。
    async fn execute(
        &self,
        repository: &Repository,
        context: &ExecutionContext,
        parameters: &Value,
    ) -> Result<Value, CapabilityError>;

    /// 审计实体：返回 `(entity_type, entity_id)`。
    fn audit_entity(&self, context: &ExecutionContext, parameters: &Value) -> (String, String);
}

/// 执行一个已注册能力。
pub async fn execute(
    repository: &Repository,
    context: &ExecutionContext,
    name: &str,
    parameters: &Value,
) -> Result<ExecutionOutcome, CapabilityError> {
    let entry = find(name).ok_or_else(|| CapabilityError::UnknownCapability(name.to_string()))?;
    let (entity_type, entity_id) = entry.executor.audit_entity(context, parameters);
    let parameters_summary = summarize(parameters);

    if !context.has_permissions(entry.spec.required_permissions) {
        let message = format!("missing required permission for capability `{name}`");
        record_failure(
            repository,
            entry,
            context,
            &entity_type,
            &entity_id,
            &parameters_summary,
            &message,
        )
        .await;
        return Err(CapabilityError::Forbidden(message));
    }

    let schema = entry.spec.parameters();
    if let Err(message) = schema::validate(&schema, parameters) {
        record_failure(
            repository,
            entry,
            context,
            &entity_type,
            &entity_id,
            &parameters_summary,
            &message,
        )
        .await;
        return Err(CapabilityError::InvalidParameters(message));
    }

    match entry
        .executor
        .execute(repository, context, parameters)
        .await
    {
        Ok(result) => Ok(ExecutionOutcome {
            capability: entry.spec.name,
            risk_level: entry.spec.risk_level,
            result,
        }),
        Err(error) => {
            record_failure(
                repository,
                entry,
                context,
                &entity_type,
                &entity_id,
                &parameters_summary,
                &error.to_string(),
            )
            .await;
            Err(error)
        }
    }
}

async fn record_failure(
    repository: &Repository,
    entry: &CapabilityEntry,
    context: &ExecutionContext,
    entity_type: &str,
    entity_id: &str,
    parameters_summary: &str,
    message: &str,
) {
    let result_payload = json!({ "error": truncate(message, 500) }).to_string();
    if let Err(error) = repository
        .record_failed_operation(
            entry.spec.name,
            Some(&context.actor_user_id),
            entity_type,
            entity_id,
            parameters_summary,
            &result_payload,
        )
        .await
    {
        tracing::error!(
            capability = entry.spec.name,
            %error,
            "failed to write capability audit record"
        );
    }
}

/// 参数摘要：序列化后截断，避免审计表被超大请求撑爆。
fn summarize(parameters: &Value) -> String {
    let text = serde_json::to_string(parameters).unwrap_or_else(|_| "{}".to_string());
    truncate(&text, 2000)
}

pub(crate) fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut truncated: String = text.chars().take(max_chars).collect();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_text_and_cuts_long_text() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("中文测试", 2), "中文…");
    }

    #[test]
    fn context_permissions_are_explicit() {
        let context = ExecutionContext::authenticated("u1", false);
        assert!(context.has_permission("order:create"));
        assert!(!context.has_permission(ELEVATED_DELETE_PERMISSION));

        let dev = ExecutionContext::authenticated("u1", true);
        assert!(dev.has_permission(ELEVATED_DELETE_PERMISSION));

        let restricted = ExecutionContext::with_permissions("u1", ["balance:read"]);
        assert!(restricted.has_permission("balance:read"));
        assert!(!restricted.has_permission("order:create"));
    }
}
