//! UI 生成模型接口：非流式生成、流式生成、运行信息与指标。

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use validator::Validate;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::model::{
    GatewayInfo, MetricsSnapshot, ModelError, StreamEvent, UiGeneration, UiRequest,
};
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::validation::ValidatedJson;

/// 生成请求。
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct GenerateRequest {
    /// 用户需求（1–32000 字符）。
    #[validate(length(min = 1, max = 32000, message = "prompt must be 1..=32000 characters"))]
    pub prompt: String,
    /// 场景编码。
    pub scene_code: Option<String>,
    /// 显式模型（必须是已注册模型）。
    pub model: Option<String>,
    /// 最大输出 token。
    pub max_tokens: Option<u32>,
    /// 采样温度。
    pub temperature: Option<f32>,
}

impl From<GenerateRequest> for UiRequest {
    fn from(value: GenerateRequest) -> Self {
        Self {
            prompt: value.prompt,
            scene_code: value.scene_code,
            model: value.model,
            max_tokens: value.max_tokens,
            temperature: value.temperature,
        }
    }
}

/// `POST /api/v1/ui/model/generate`：非流式生成单文件 HTML。
pub async fn generate(
    State(state): State<AppState>,
    _user: AuthUser,
    ValidatedJson(request): ValidatedJson<GenerateRequest>,
) -> Result<Json<ApiResponse<UiGeneration>>, AppError> {
    let generation = state
        .model
        .generate_ui(request.into())
        .await
        .map_err(AppError::from)?;
    Ok(Json(ApiResponse::success(generation)))
}

/// `POST /api/v1/ui/model/stream`：SSE 流式生成。
pub async fn stream(
    State(state): State<AppState>,
    _user: AuthUser,
    ValidatedJson(request): ValidatedJson<GenerateRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let events = state
        .model
        .stream_ui(request.into())
        .await
        .map_err(AppError::from)?;
    let stream = events.map(|item| Ok(to_sse_event(item)));
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

/// `GET /api/v1/ui/model/info`：模型接入运行信息。
pub async fn info(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Json<ApiResponse<GatewayInfo>> {
    Json(ApiResponse::success(state.model.info()))
}

/// `GET /api/v1/ui/model/metrics`：进程内调用指标快照。
pub async fn metrics(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Json<ApiResponse<MetricsSnapshot>> {
    Json(ApiResponse::success(state.model.snapshot()))
}

fn to_sse_event(item: Result<StreamEvent, ModelError>) -> Event {
    match item {
        Ok(StreamEvent::Meta { model }) => Event::default()
            .event("meta")
            .data(json!({ "model": model }).to_string()),
        Ok(StreamEvent::Delta { content }) => Event::default()
            .event("delta")
            .data(json!({ "content": content }).to_string()),
        Ok(StreamEvent::Done {
            model,
            usage,
            latency_ms,
            estimated_cost_usd,
            finish_reason,
        }) => Event::default().event("done").data(
            json!({
                "model": model,
                "usage": usage,
                "latency_ms": latency_ms,
                "estimated_cost_usd": estimated_cost_usd,
                "finish_reason": finish_reason,
            })
            .to_string(),
        ),
        Err(error) => Event::default().event("error").data(
            json!({
                "code": error_code(&error),
                "msg": error.to_string(),
            })
            .to_string(),
        ),
    }
}

fn error_code(error: &ModelError) -> i32 {
    match error {
        ModelError::InvalidRequest(_) | ModelError::UnknownModel(_) => 40000,
        ModelError::QueueFull => 42900,
        ModelError::Disabled | ModelError::QueueTimeout => 50300,
        ModelError::Timeout => 50400,
        ModelError::Upstream { .. } | ModelError::Protocol(_) | ModelError::InvalidOutput(_) => {
            50200
        }
    }
}

impl From<ModelError> for AppError {
    fn from(error: ModelError) -> Self {
        match error {
            ModelError::Disabled => AppError::new(
                crate::error::ErrorCode::ServiceUnavailable,
                "model service is disabled",
            ),
            ModelError::InvalidRequest(message) => AppError::invalid_param(message),
            ModelError::UnknownModel(model) => {
                AppError::invalid_param(format!("unknown model: {model}"))
            }
            ModelError::QueueFull => AppError::rate_limited(1),
            ModelError::QueueTimeout => AppError::new(
                crate::error::ErrorCode::ServiceUnavailable,
                "model queue is busy, retry later",
            ),
            ModelError::Timeout => AppError::timeout(),
            ModelError::Upstream { status: 429, .. } => AppError::rate_limited(1),
            ModelError::Upstream {
                status, message, ..
            } => AppError::bad_gateway(format!("upstream status {status}: {message}")),
            ModelError::Protocol(message) => AppError::bad_gateway(message),
            ModelError::InvalidOutput(message) => AppError::bad_gateway(message),
        }
    }
}
