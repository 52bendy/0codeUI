//! 原子能力声明式注册表（REQ-202610-1178）。
//!
//! 每个能力通过 [`CapabilitySpec`] 声明：
//! - 名称与描述（可直接生成 function / tool 定义）；
//! - 参数 JSON Schema（执行器据此做统一参数校验）；
//! - 返回结构说明；
//! - 风险等级与所需权限；
//! - 审计实体信息。
//!
//! 新增能力只需实现 `CapabilityExecutor` 并在 [`super::CAPABILITIES`] 注册，
//! 不需要修改调度核心。

use serde::Serialize;
use serde_json::Value;

/// 能力风险等级（决定是否需要二次确认，供 REQ-202610-1179 消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    /// 读操作：可自动执行。
    Read,
    /// 写操作：必须二次确认。
    Write,
    /// 删操作：必须二次确认。
    Delete,
}

/// 能力声明（静态、只读）。
#[derive(Debug, Clone, Copy)]
pub struct CapabilitySpec {
    /// 能力名称（function / tool 名称）。
    pub name: &'static str,
    /// 人类可读描述（同时用于 function 定义）。
    pub description: &'static str,
    /// 风险等级。
    pub risk_level: RiskLevel,
    /// 执行所需权限。
    pub required_permissions: &'static [&'static str],
    /// 参数 JSON Schema（对象）。
    pub parameters_schema: &'static str,
    /// 返回结构 JSON Schema（对象）。
    pub returns_schema: &'static str,
}

impl CapabilitySpec {
    /// 解析参数 Schema。
    pub(crate) fn parameters(&self) -> Value {
        parse_schema(self.parameters_schema)
    }

    /// 解析返回 Schema。
    pub(crate) fn returns(&self) -> Value {
        parse_schema(self.returns_schema)
    }
}

/// 面向 Agent 的能力描述（`GET /api/v1/capabilities` 的列表项）。
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityDescriptor {
    /// 能力名称。
    pub name: &'static str,
    /// 能力描述。
    pub description: &'static str,
    /// 风险等级。
    pub risk_level: RiskLevel,
    /// 所需权限。
    pub required_permissions: &'static [&'static str],
    /// 参数 JSON Schema。
    pub parameters: Value,
    /// 返回结构说明。
    pub returns: Value,
    /// OpenAI function / tool 定义（name + description + parameters）。
    pub function: FunctionDefinition,
}

/// OpenAI function / tool 定义（`{ name, description, parameters }`）。
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct FunctionDefinition(Value);

impl FunctionDefinition {
    pub(crate) fn new(name: &'static str, description: &'static str, parameters: Value) -> Self {
        Self(serde_json::json!({
            "name": name,
            "description": description,
            "parameters": parameters,
        }))
    }
}

impl std::ops::Index<&str> for FunctionDefinition {
    type Output = Value;

    fn index(&self, index: &str) -> &Self::Output {
        &self.0[index]
    }
}

fn parse_schema(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|error| {
        tracing::error!(schema = raw, %error, "invalid static capability schema");
        Value::Null
    })
}

/// `create_order` 声明。
pub(crate) const CREATE_ORDER_SPEC: CapabilitySpec = CapabilitySpec {
    name: "create_order",
    description: "创建订单：按商品快照计价并写入订单、订单项与审计记录。",
    risk_level: RiskLevel::Write,
    required_permissions: &["order:create"],
    parameters_schema: r#"{
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "user_id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 64,
                "pattern": "^[A-Za-z0-9._:-]+$",
                "description": "下单用户；缺省为当前登录用户，显式提供时必须与登录用户一致"
            },
            "order_id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 64,
                "pattern": "^[A-Za-z0-9._:-]+$",
                "description": "可选订单号；缺省由服务端生成"
            },
            "shipping_address": {
                "type": "string",
                "minLength": 1,
                "maxLength": 200,
                "description": "收货地址"
            },
            "items": {
                "type": "array",
                "minItems": 1,
                "maxItems": 100,
                "description": "订单项（1~100 项）",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "product_id": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 64,
                            "pattern": "^[A-Za-z0-9._:-]+$",
                            "description": "商品 ID"
                        },
                        "qty": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 999,
                            "description": "购买数量"
                        }
                    },
                    "required": ["product_id", "qty"]
                }
            }
        },
        "required": ["items"]
    }"#,
    returns_schema: r#"{
        "type": "object",
        "description": "创建成功的订单",
        "properties": {
            "id": {"type": "string"},
            "user_id": {"type": "string"},
            "status": {"type": "string"},
            "total_amount": {"type": "number"},
            "currency": {"type": "string"}
        }
    }"#,
};

/// `query_user_balance` 声明。
pub(crate) const QUERY_USER_BALANCE_SPEC: CapabilitySpec = CapabilitySpec {
    name: "query_user_balance",
    description: "查询用户余额（读操作，自动执行）。",
    risk_level: RiskLevel::Read,
    required_permissions: &["balance:read"],
    parameters_schema: r#"{
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "user_id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 64,
                "pattern": "^[A-Za-z0-9._:-]+$",
                "description": "用户标识；缺省为当前登录用户，显式提供时必须与登录用户一致"
            }
        },
        "required": []
    }"#,
    returns_schema: r#"{
        "type": "object",
        "description": "用户余额",
        "properties": {
            "user_id": {"type": "string"},
            "balance": {"type": "number"},
            "currency": {"type": "string"}
        }
    }"#,
};

/// `update_item_status` 声明。
pub(crate) const UPDATE_ITEM_STATUS_SPEC: CapabilitySpec = CapabilitySpec {
    name: "update_item_status",
    description: "更新订单项状态（写操作，需要二次确认）。",
    risk_level: RiskLevel::Write,
    required_permissions: &["order_item:update"],
    parameters_schema: r#"{
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "item_id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128,
                "pattern": "^[A-Za-z0-9._:-]+$",
                "description": "订单项 ID"
            },
            "status": {
                "type": "string",
                "enum": ["NORMAL", "SHIPPED", "REFUNDED", "CANCELLED"],
                "description": "目标状态"
            }
        },
        "required": ["item_id", "status"]
    }"#,
    returns_schema: r#"{
        "type": "object",
        "description": "更新后的订单项",
        "properties": {
            "id": {"type": "string"},
            "order_id": {"type": "string"},
            "status": {"type": "string"}
        }
    }"#,
};

/// `delete_record` 声明。
pub(crate) const DELETE_RECORD_SPEC: CapabilitySpec = CapabilitySpec {
    name: "delete_record",
    description: "删除指定实体（删操作，需要二次确认；商品与审计记录需要更高权限）。",
    risk_level: RiskLevel::Delete,
    required_permissions: &["record:delete"],
    parameters_schema: r#"{
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "entity_type": {
                "type": "string",
                "enum": ["order", "order_item", "product", "operation_record"],
                "description": "实体类型"
            },
            "entity_id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128,
                "pattern": "^[A-Za-z0-9._:-]+$",
                "description": "实体主键"
            }
        },
        "required": ["entity_type", "entity_id"]
    }"#,
    returns_schema: r#"{
        "type": "object",
        "description": "删除结果",
        "properties": {
            "entity_type": {"type": "string"},
            "entity_id": {"type": "string"},
            "deleted": {"type": "boolean"}
        }
    }"#,
};
