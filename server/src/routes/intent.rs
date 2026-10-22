//! 意图执行端点（PRD §5.2）。

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;

use crate::error::AppError;
use crate::intent::model::{ActionType, IntentExecuteRequest, IntentExecuteResponse};
use crate::middleware::auth::AuthUser;
use crate::response::ApiResponse;
use crate::state::AppState;

/// `POST /api/v1/intent/execute`
///
/// 输入自然语言或 UI 事件树；读类意图直接执行，写 / 删类返回确认卡片。
pub async fn execute(
    State(state): State<AppState>,
    auth: AuthUser,
    payload: Result<Json<IntentExecuteRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<IntentExecuteResponse>>, AppError> {
    let Json(request) = payload.map_err(|rejection| {
        AppError::invalid_param(format!("invalid request body: {}", rejection.body_text()))
    })?;
    let mut response = state.intent_service.execute(&auth.user_id, request).await?;

    // 写 / 删类意图只下发确认卡片；真正的执行必须走 confirm 端点。
    if response.action_type == ActionType::RequireConfirmation {
        let prompt = response
            .confirmation_prompt
            .clone()
            .unwrap_or_else(|| "确认执行该操作吗？".to_string());
        let confirmation = state
            .confirmation_service
            .create(
                &auth.user_id,
                &response.session_id,
                &response.intent_summary,
                &response.proposed_changes,
                &prompt,
            )
            .await?;
        response.confirmation = Some(confirmation);
    }

    Ok(Json(ApiResponse::success(response)))
}
