//! 原子能力注册中心端点（REQ-202610-1178）。
//!
//! - `GET  /api/v1/capabilities`：能力清单（含 OpenAI function 定义）；
//! - `POST /api/v1/capabilities/{name}/execute`：执行指定能力。

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::capability::{self, CapabilityDescriptor, ExecutionContext, RiskLevel};
use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::repository::Repository;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::validation::ValidatedJson;

/// `GET /api/v1/capabilities` 响应数据。
#[derive(Debug, Serialize)]
pub struct CapabilityListData {
    /// 能力清单。
    pub capabilities: Vec<CapabilityDescriptor>,
    /// 能力数量。
    pub count: usize,
}

/// 能力执行响应数据。
#[derive(Debug, Serialize)]
pub struct CapabilityExecutionData {
    /// 能力名称。
    pub capability: &'static str,
    /// 风险等级。
    pub risk_level: RiskLevel,
    /// 能力返回值。
    pub result: Value,
}

/// 能力执行请求体。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteCapabilityRequest {
    /// 能力参数（结构由能力自身的 JSON Schema 声明）。
    #[serde(default = "empty_parameters")]
    pub parameters: Value,
}

fn empty_parameters() -> Value {
    Value::Object(serde_json::Map::new())
}

impl Validate for ExecuteCapabilityRequest {
    fn validate(&self) -> Result<(), ValidationErrors> {
        if self.parameters.is_object() {
            return Ok(());
        }
        let mut errors = ValidationErrors::new();
        errors.add(
            "parameters",
            ValidationError::new("parameters must be a JSON object"),
        );
        Err(errors)
    }
}

/// `GET /api/v1/capabilities`
///
/// 返回全部已注册能力，`function` 字段可直接作为 Agent 的 tool 定义。
pub async fn list() -> Json<ApiResponse<CapabilityListData>> {
    let capabilities = capability::descriptors();
    let count = capabilities.len();
    Json(ApiResponse::success(CapabilityListData {
        capabilities,
        count,
    }))
}

/// `POST /api/v1/capabilities/{name}/execute`
///
/// 执行链路：权限校验 → 参数 Schema 校验 → 执行 → 审计。
pub async fn execute(
    State(state): State<AppState>,
    AuthUser { user_id }: AuthUser,
    Path(name): Path<String>,
    ValidatedJson(request): ValidatedJson<ExecuteCapabilityRequest>,
) -> Result<Json<ApiResponse<CapabilityExecutionData>>, AppError> {
    let repository = Repository::from_db(&state.db);
    let context = ExecutionContext::authenticated(user_id, state.config.auth_dev_mode);
    let outcome = capability::execute(&repository, &context, &name, &request.parameters).await?;

    Ok(Json(ApiResponse::success(CapabilityExecutionData {
        capability: outcome.capability,
        risk_level: outcome.risk_level,
        result: outcome.result,
    })))
}
