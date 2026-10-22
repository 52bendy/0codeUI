//! 首批四个原子能力的执行器实现。
//!
//! 每个执行器只负责「把校验后的参数映射为仓储调用」与审计实体识别；
//! 权限、参数 Schema 校验、失败审计由调度核心统一处理。

use async_trait::async_trait;
use serde_json::Value;

use crate::models::{CreateOrderInput, OrderItem};
use crate::repository::Repository;

use super::executor::{
    CapabilityError, CapabilityExecutor, ExecutionContext, ELEVATED_DELETE_PERMISSION,
};

/// `create_order` 执行器。
pub(crate) struct CreateOrderExecutor;

#[async_trait]
impl CapabilityExecutor for CreateOrderExecutor {
    async fn execute(
        &self,
        repository: &Repository,
        context: &ExecutionContext,
        parameters: &Value,
    ) -> Result<Value, CapabilityError> {
        let user_id = scoped_user_id(context, parameters)?;
        let mut value = parameters.clone();
        let object = value.as_object_mut().ok_or_else(|| {
            CapabilityError::InvalidParameters("parameters must be an object".to_string())
        })?;
        object.insert("user_id".to_string(), Value::String(user_id));

        let input: CreateOrderInput = serde_json::from_value(value).map_err(|error| {
            CapabilityError::InvalidParameters(format!("invalid create_order parameters: {error}"))
        })?;
        let order = repository.create_order(&input).await?;
        to_json(order)
    }

    fn audit_entity(&self, _context: &ExecutionContext, parameters: &Value) -> (String, String) {
        (
            "order".to_string(),
            string_param(parameters, "order_id").unwrap_or_else(|| "generated".to_string()),
        )
    }
}

/// `query_user_balance` 执行器。
pub(crate) struct QueryUserBalanceExecutor;

#[async_trait]
impl CapabilityExecutor for QueryUserBalanceExecutor {
    async fn execute(
        &self,
        repository: &Repository,
        context: &ExecutionContext,
        parameters: &Value,
    ) -> Result<Value, CapabilityError> {
        let user_id = scoped_user_id(context, parameters)?;
        let balance = repository.query_user_balance(&user_id).await?;
        to_json(balance)
    }

    fn audit_entity(&self, context: &ExecutionContext, parameters: &Value) -> (String, String) {
        (
            "user".to_string(),
            string_param(parameters, "user_id").unwrap_or_else(|| context.actor_user_id.clone()),
        )
    }
}

/// `update_item_status` 执行器。
pub(crate) struct UpdateItemStatusExecutor;

#[async_trait]
impl CapabilityExecutor for UpdateItemStatusExecutor {
    async fn execute(
        &self,
        repository: &Repository,
        context: &ExecutionContext,
        parameters: &Value,
    ) -> Result<Value, CapabilityError> {
        let item_id = required_string(parameters, "item_id")?;
        let status = required_string(parameters, "status")?;
        ensure_item_owned(repository, context, item_id).await?;

        let item = repository
            .update_item_status(Some(&context.actor_user_id), item_id, status)
            .await?;
        to_json(item)
    }

    fn audit_entity(&self, _context: &ExecutionContext, parameters: &Value) -> (String, String) {
        (
            "order_item".to_string(),
            string_param(parameters, "item_id").unwrap_or_else(|| "-".to_string()),
        )
    }
}

/// `delete_record` 执行器。
pub(crate) struct DeleteRecordExecutor;

#[async_trait]
impl CapabilityExecutor for DeleteRecordExecutor {
    async fn execute(
        &self,
        repository: &Repository,
        context: &ExecutionContext,
        parameters: &Value,
    ) -> Result<Value, CapabilityError> {
        let entity_type = required_string(parameters, "entity_type")?;
        let entity_id = required_string(parameters, "entity_id")?;

        match entity_type {
            "order" => {
                let order = repository.find_order(entity_id).await?;
                if order.user_id != context.actor_user_id {
                    return Err(CapabilityError::Forbidden(
                        "order does not belong to the authenticated user".to_string(),
                    ));
                }
            }
            "order_item" => {
                ensure_item_owned(repository, context, entity_id).await?;
            }
            "product" | "operation_record" => {
                if !context.has_permission(ELEVATED_DELETE_PERMISSION) {
                    return Err(CapabilityError::Forbidden(format!(
                        "deleting `{entity_type}` requires permission `{ELEVATED_DELETE_PERMISSION}`"
                    )));
                }
            }
            other => {
                return Err(CapabilityError::InvalidParameters(format!(
                    "unsupported entity type `{other}`"
                )));
            }
        }

        let deleted = repository
            .delete_record(Some(&context.actor_user_id), entity_type, entity_id)
            .await?;
        to_json(deleted)
    }

    fn audit_entity(&self, _context: &ExecutionContext, parameters: &Value) -> (String, String) {
        (
            string_param(parameters, "entity_type").unwrap_or_else(|| "record".to_string()),
            string_param(parameters, "entity_id").unwrap_or_else(|| "-".to_string()),
        )
    }
}

/// 解析目标用户：缺省为操作人，显式提供时必须与操作人一致。
fn scoped_user_id(
    context: &ExecutionContext,
    parameters: &Value,
) -> Result<String, CapabilityError> {
    match parameters.get("user_id") {
        None | Some(Value::Null) => Ok(context.actor_user_id.clone()),
        Some(Value::String(user_id)) if user_id == &context.actor_user_id => Ok(user_id.clone()),
        Some(Value::String(user_id)) => Err(CapabilityError::Forbidden(format!(
            "user_id `{user_id}` does not match the authenticated user"
        ))),
        Some(_) => Err(CapabilityError::InvalidParameters(
            "user_id must be a string".to_string(),
        )),
    }
}

/// 校验订单项归属。
async fn ensure_item_owned(
    repository: &Repository,
    context: &ExecutionContext,
    item_id: &str,
) -> Result<OrderItem, CapabilityError> {
    let item = repository.find_order_item(item_id).await?;
    let order = repository.find_order(&item.order_id).await?;
    if order.user_id != context.actor_user_id {
        return Err(CapabilityError::Forbidden(
            "order item does not belong to the authenticated user".to_string(),
        ));
    }
    Ok(item)
}

fn required_string<'a>(parameters: &'a Value, key: &str) -> Result<&'a str, CapabilityError> {
    parameters
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| CapabilityError::InvalidParameters(format!("{key} must be a string")))
}

fn string_param(parameters: &Value, key: &str) -> Option<String> {
    parameters
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn to_json<T: serde::Serialize>(value: T) -> Result<Value, CapabilityError> {
    serde_json::to_value(value).map_err(|error| CapabilityError::Internal(error.to_string()))
}
