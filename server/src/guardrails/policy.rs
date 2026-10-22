//! 风险分级策略（PRD §3.3 安全风控拦截）。
//!
//! 规则：
//! - `READ`：自动执行，不生成确认卡片；
//! - `WRITE` / `DELETE`：一律生成确认卡片，未确认绝不执行；
//! - 多个动作按最高风险等级合并（`DELETE` > `WRITE` > `READ`）；
//! - 未注册的能力直接拒绝，避免「未知即放行」。
//!
//! 当前风险元数据来自 [`crate::intent::capabilities`] 的静态目录；
//! REQ-202610-1178 的能力注册中心落地后，只需把 [`lookup`] 替换为
//! 注册表查询，其余策略与调用方无需改动。

use serde_json::Value;

use crate::error::AppError;
use crate::intent::capabilities;
use crate::intent::model::{ProposedChanges, RiskLevel};

/// 风险分级结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskAssessment {
    /// 合并后的最高风险等级。
    pub level: RiskLevel,
    /// 是否需要二次确认。
    pub requires_confirmation: bool,
}

impl RiskAssessment {
    /// 风险等级中文标签（用于确认文案与审计）。
    pub const fn label(self) -> &'static str {
        risk_label(self.level)
    }
}

/// 风险等级中文标签。
pub const fn risk_label(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Read => "只读",
        RiskLevel::Write => "写操作",
        RiskLevel::Delete => "删除操作",
    }
}

/// 风险严重度（用于合并）。
const fn severity(level: RiskLevel) -> u8 {
    match level {
        RiskLevel::Read => 0,
        RiskLevel::Write => 1,
        RiskLevel::Delete => 2,
    }
}

/// 读类能力是否可自动执行。
pub const fn requires_confirmation(level: RiskLevel) -> bool {
    !matches!(level, RiskLevel::Read)
}

/// 查询能力风险等级；未注册能力返回 `None`。
pub fn lookup(capability: &str) -> Option<RiskLevel> {
    capabilities::spec(capability).map(|spec| spec.risk_level)
}

/// 对单个能力做风险评估。
pub fn assess_capability(capability: &str) -> Result<RiskAssessment, AppError> {
    let level = lookup(capability)
        .ok_or_else(|| AppError::invalid_param(format!("unknown capability `{capability}`")))?;
    Ok(RiskAssessment {
        level,
        requires_confirmation: requires_confirmation(level),
    })
}

/// 对一组能力做风险评估（取最高风险）。
pub fn assess_capabilities<'a>(
    capabilities: impl IntoIterator<Item = &'a str>,
) -> Result<RiskAssessment, AppError> {
    let mut level = RiskLevel::Read;
    let mut found = false;
    for capability in capabilities {
        let current = lookup(capability)
            .ok_or_else(|| AppError::invalid_param(format!("unknown capability `{capability}`")))?;
        if !found || severity(current) > severity(level) {
            level = current;
            found = true;
        }
    }
    if !found {
        return Err(AppError::invalid_param(
            "at least one capability is required",
        ));
    }
    Ok(RiskAssessment {
        level,
        requires_confirmation: requires_confirmation(level),
    })
}

/// 对 [`ProposedChanges`] 做风险评估。
pub fn assess_changes(changes: &ProposedChanges) -> Result<RiskAssessment, AppError> {
    assess_capabilities(actions_of(changes).into_iter().map(|(name, _)| name))
}

/// 把 [`ProposedChanges`] 摊平为动作列表。
pub fn actions_of(changes: &ProposedChanges) -> Vec<(&str, &Value)> {
    let mut actions = Vec::with_capacity(1 + changes.additional_calls.as_ref().map_or(0, Vec::len));
    actions.push((changes.target_capability.as_str(), &changes.parameters));
    if let Some(additional) = &changes.additional_calls {
        actions.extend(
            additional
                .iter()
                .map(|call| (call.target_capability.as_str(), &call.parameters)),
        );
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::model::ProposedCall;
    use serde_json::json;

    #[test]
    fn read_capabilities_do_not_require_confirmation() {
        let assessment = assess_capability("query_user_balance").unwrap();
        assert_eq!(assessment.level, RiskLevel::Read);
        assert!(!assessment.requires_confirmation);
    }

    #[test]
    fn write_and_delete_require_confirmation() {
        assert!(
            assess_capability("update_order_address")
                .unwrap()
                .requires_confirmation
        );
        assert!(
            assess_capability("delete_record")
                .unwrap()
                .requires_confirmation
        );
    }

    #[test]
    fn highest_risk_wins() {
        let changes = ProposedChanges {
            target_capability: "query_user_balance".to_string(),
            parameters: json!({}),
            additional_calls: Some(vec![
                ProposedCall {
                    target_capability: "update_order_address".to_string(),
                    parameters: json!({"order_id": "ORD_1", "new_address": "A"}),
                },
                ProposedCall {
                    target_capability: "delete_record".to_string(),
                    parameters: json!({"entity_type": "order", "entity_id": "ORD_1"}),
                },
            ]),
        };
        let assessment = assess_changes(&changes).unwrap();
        assert_eq!(assessment.level, RiskLevel::Delete);
        assert!(assessment.requires_confirmation);
    }

    #[test]
    fn unknown_capability_is_rejected() {
        assert!(assess_capability("drop_database").is_err());
    }

    #[test]
    fn actions_are_flattened_in_order() {
        let changes = ProposedChanges {
            target_capability: "a".to_string(),
            parameters: json!({"n": 1}),
            additional_calls: Some(vec![ProposedCall {
                target_capability: "b".to_string(),
                parameters: json!({"n": 2}),
            }]),
        };
        let actions = actions_of(&changes);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].0, "a");
        assert_eq!(actions[1].0, "b");
    }
}
