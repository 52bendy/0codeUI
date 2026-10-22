//! 动态生成管线端点（REQ-202610-1185）。
//!
//! - `POST /api/v1/ui/generate/dynamic`：与 `POST /api/v1/ui/generate` 同构的
//!   请求体，走「画像 → Prompt → 模型 → 校验 → 混淆」动态链路；失败自动降级
//!   到静态模板并返回 `degraded=true`。
//! - `GET /api/v1/ui/pipeline/metrics`：降级率、缓存命中、延迟分位等快照。
//! - `GET /api/v1/ui/pipeline/info`：时间预算、缓存与混淆配置。

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::Value;
use validator::Validate;

use crate::error::{AppError, ErrorCode};
use crate::middleware::auth::AuthUser;
use crate::pipeline::{
    PipelineError, PipelineInfo, PipelineMetricsSnapshot, PipelineOutcome, PipelineRequest,
    PipelineStreamEvent,
};
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::ui::device::DeviceInfo;
use crate::validation::ValidatedJson;

/// 动态生成请求（与 03 接口 `UiGenerateRequest` 字段对齐，额外支持提示词 / 模型 / 会话）。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct DynamicGenerateRequest {
    /// 调用方用户标识。
    #[validate(length(min = 1, max = 64, message = "user_id must be 1..=64 characters"))]
    pub user_id: String,
    /// 场景编码。
    #[validate(length(min = 1, max = 64, message = "scene_code must be 1..=64 characters"))]
    pub scene_code: String,
    /// 设备信息。
    pub device_info: DeviceInfo,
    /// 场景业务数据。
    pub business_data: Value,
    /// 可选用户意图描述。
    #[validate(length(max = 32_000, message = "prompt must be at most 32000 characters"))]
    pub prompt: Option<String>,
    /// 可选模型名（必须是网关已注册模型）。
    #[validate(length(max = 128, message = "model must be at most 128 characters"))]
    pub model: Option<String>,
    /// 可选会话标识（混淆类名映射）。
    #[validate(length(max = 128, message = "session_id must be at most 128 characters"))]
    pub session_id: Option<String>,
    /// 跳过缓存。
    #[serde(default)]
    pub bypass_cache: bool,
}

/// `POST /api/v1/ui/generate/dynamic`
pub async fn generate_dynamic(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(payload): ValidatedJson<DynamicGenerateRequest>,
) -> Result<Json<ApiResponse<PipelineOutcome>>, AppError> {
    crate::routes::ui::ensure_user_scope(&state, &auth, &payload.user_id)?;

    let request = into_pipeline_request(payload);
    let outcome = state
        .pipeline
        .generate(request)
        .await
        .map_err(map_pipeline_error)?;
    Ok(Json(ApiResponse::success(outcome)))
}

/// `POST /api/v1/ui/generate/dynamic/stream`：SSE 渐进式下发。
///
/// 连接建立后立即推送预渲染的静态兜底页（`fallback`），随后推送动态结果
/// （`result`）或降级结果（`degraded`）。客户端先渲染兜底页，收到动态结果后
/// 再替换，从而把首屏感知压到模板渲染耗时（毫秒级）。
pub async fn generate_dynamic_stream(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(payload): ValidatedJson<DynamicGenerateRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    crate::routes::ui::ensure_user_scope(&state, &auth, &payload.user_id)?;
    let request = into_pipeline_request(payload);
    let preview = state
        .pipeline
        .render_fallback_preview(&request)
        .map_err(map_pipeline_error)?;

    let pipeline = state.pipeline.clone();
    let (sender, receiver) = tokio::sync::mpsc::channel::<PipelineStreamEvent>(8);
    tokio::spawn(async move {
        if sender
            .send(PipelineStreamEvent::Fallback { preview })
            .await
            .is_err()
        {
            return;
        }
        match pipeline.generate(request).await {
            Ok(outcome) if outcome.degraded => {
                let _ = sender.send(PipelineStreamEvent::Degraded { outcome }).await;
            }
            Ok(outcome) => {
                let _ = sender.send(PipelineStreamEvent::Result { outcome }).await;
            }
            Err(error) => {
                let _ = sender
                    .send(PipelineStreamEvent::Error {
                        code: pipeline_error_code(&error),
                        message: error.to_string(),
                    })
                    .await;
            }
        }
    });

    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| {
            let name = stream_event_name(&event);
            let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
            (Ok(Event::default().event(name).data(data)), receiver)
        })
    });
    Ok(Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

fn into_pipeline_request(payload: DynamicGenerateRequest) -> PipelineRequest {
    let device = crate::ui_template::DeviceInfo {
        screen_width: payload.device_info.screen_width,
        screen_height: payload.device_info.screen_height,
        platform: payload.device_info.platform,
    };
    PipelineRequest {
        user_id: payload.user_id,
        scene_code: payload.scene_code,
        device,
        business_data: payload.business_data,
        prompt: payload.prompt,
        model: payload.model,
        session_id: payload.session_id,
        bypass_cache: payload.bypass_cache,
    }
}

fn stream_event_name(event: &PipelineStreamEvent) -> &'static str {
    match event {
        PipelineStreamEvent::Fallback { .. } => "fallback",
        PipelineStreamEvent::Result { .. } => "result",
        PipelineStreamEvent::Degraded { .. } => "degraded",
        PipelineStreamEvent::Error { .. } => "error",
    }
}

fn pipeline_error_code(error: &PipelineError) -> i32 {
    match error {
        PipelineError::SceneNotFound(_) => ErrorCode::SceneNotFound.as_i32(),
        PipelineError::InvalidRequest(_) | PipelineError::InvalidBusinessData(_) => {
            ErrorCode::InvalidParam.as_i32()
        }
        PipelineError::FallbackFailed(_) | PipelineError::Internal(_) => {
            ErrorCode::Internal.as_i32()
        }
    }
}

/// `GET /api/v1/ui/pipeline/metrics`
pub async fn metrics(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Json<ApiResponse<PipelineMetricsSnapshot>> {
    Json(ApiResponse::success(state.pipeline.metrics().snapshot()))
}

/// `GET /api/v1/ui/pipeline/info`
pub async fn info(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Json<ApiResponse<PipelineInfo>> {
    Json(ApiResponse::success(state.pipeline.info()))
}

fn map_pipeline_error(error: PipelineError) -> AppError {
    match error {
        PipelineError::SceneNotFound(message) => AppError::new(ErrorCode::SceneNotFound, message),
        PipelineError::InvalidRequest(message) | PipelineError::InvalidBusinessData(message) => {
            AppError::invalid_param(message)
        }
        PipelineError::FallbackFailed(message) => {
            AppError::internal(format!("fallback render failed: {message}"))
        }
        PipelineError::Internal(message) => AppError::internal(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_rejects_oversized_prompt() {
        let request = DynamicGenerateRequest {
            user_id: "u1".to_string(),
            scene_code: "ORDER_DETAIL_PAGE".to_string(),
            device_info: DeviceInfo {
                screen_width: 390,
                screen_height: 844,
                platform: "web".to_string(),
            },
            business_data: serde_json::json!({"order_id": "ORD_1"}),
            prompt: Some("x".repeat(32_001)),
            model: None,
            session_id: None,
            bypass_cache: false,
        };
        assert!(request.validate().is_err());
    }
}
