//! 原子能力注册中心（REQ-202610-1178）。
//!
//! 设计要点：
//! - **声明式**：每个能力由 [`CapabilitySpec`] 描述名称、参数 Schema、
//!   返回结构、风险等级与所需权限；
//! - **可扩展**：新增能力只需实现 [`CapabilityExecutor`] 并在
//!   [`CAPABILITIES`] 追加一条注册项，调度核心（[`execute`]）无需改动；
//! - **可消费**：[`descriptors`] 输出带 OpenAI function 定义的清单，
//!   供意图引擎 / Agent 生成工具调用（REQ-202610-1177 / 1179）。
//!
//! 执行链路固定为：权限校验 → 参数 Schema 校验 → 执行 → 审计。

mod builtin;
mod executor;
mod schema;
mod spec;

pub use executor::{execute, CapabilityError, ExecutionContext, ExecutionOutcome};
pub use spec::{CapabilityDescriptor, CapabilitySpec, FunctionDefinition, RiskLevel};

pub(crate) use executor::CapabilityExecutor;
pub(crate) use spec::{
    CREATE_ORDER_SPEC, DELETE_RECORD_SPEC, QUERY_USER_BALANCE_SPEC, UPDATE_ITEM_STATUS_SPEC,
};

use builtin::{
    CreateOrderExecutor, DeleteRecordExecutor, QueryUserBalanceExecutor, UpdateItemStatusExecutor,
};

/// 一条能力注册项：声明 + 执行器。
pub(crate) struct CapabilityEntry {
    /// 能力声明。
    pub(crate) spec: CapabilitySpec,
    /// 执行器实例。
    pub(crate) executor: &'static dyn CapabilityExecutor,
}

static CREATE_ORDER_EXECUTOR: CreateOrderExecutor = CreateOrderExecutor;
static QUERY_USER_BALANCE_EXECUTOR: QueryUserBalanceExecutor = QueryUserBalanceExecutor;
static UPDATE_ITEM_STATUS_EXECUTOR: UpdateItemStatusExecutor = UpdateItemStatusExecutor;
static DELETE_RECORD_EXECUTOR: DeleteRecordExecutor = DeleteRecordExecutor;

/// 首批原子能力注册表（顺序即对外清单顺序）。
static CAPABILITIES: &[CapabilityEntry] = &[
    CapabilityEntry {
        spec: CREATE_ORDER_SPEC,
        executor: &CREATE_ORDER_EXECUTOR,
    },
    CapabilityEntry {
        spec: QUERY_USER_BALANCE_SPEC,
        executor: &QUERY_USER_BALANCE_EXECUTOR,
    },
    CapabilityEntry {
        spec: UPDATE_ITEM_STATUS_SPEC,
        executor: &UPDATE_ITEM_STATUS_EXECUTOR,
    },
    CapabilityEntry {
        spec: DELETE_RECORD_SPEC,
        executor: &DELETE_RECORD_EXECUTOR,
    },
];

/// 按名称查找注册项。
pub(crate) fn find(name: &str) -> Option<&'static CapabilityEntry> {
    CAPABILITIES.iter().find(|entry| entry.spec.name == name)
}

/// 能力清单（供 `GET /api/v1/capabilities` 与 Agent 消费）。
pub fn descriptors() -> Vec<CapabilityDescriptor> {
    CAPABILITIES
        .iter()
        .map(|entry| {
            let parameters = entry.spec.parameters();
            CapabilityDescriptor {
                name: entry.spec.name,
                description: entry.spec.description,
                risk_level: entry.spec.risk_level,
                required_permissions: entry.spec.required_permissions,
                returns: entry.spec.returns(),
                function: FunctionDefinition::new(
                    entry.spec.name,
                    entry.spec.description,
                    parameters.clone(),
                ),
                parameters,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_are_unique_and_schemas_parse() {
        let names: Vec<&str> = CAPABILITIES.iter().map(|entry| entry.spec.name).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());

        for descriptor in descriptors() {
            assert_ne!(descriptor.parameters, serde_json::Value::Null);
            assert_ne!(descriptor.returns, serde_json::Value::Null);
        }
    }
}
