//! 二次确认端点（REQ-202610-1179）。
//!
//! - `POST /api/v1/intent/confirm`：确认并执行（仅执行一次）；
//! - `POST /api/v1/intent/reject`：拒绝 / 取消，不执行任何动作。

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;

use crate::error::AppError;
use crate::guardrails::model::{ConfirmRequest, ConfirmationDecision, RejectRequest};
use crate::middleware::auth::AuthUser;
use crate::response::ApiResponse;
use crate::state::AppState;

/// `POST /api/v1/intent/confirm`
pub async fn confirm(
    State(state): State<AppState>,
    auth: AuthUser,
    payload: Result<Json<ConfirmRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ConfirmationDecision>>, AppError> {
    let Json(request) = parse_body(payload)?;
    let decision = state
        .confirmation_service
        .confirm(&auth.user_id, request)
        .await?;
    Ok(Json(ApiResponse::success(decision)))
}

/// `POST /api/v1/intent/reject`
pub async fn reject(
    State(state): State<AppState>,
    auth: AuthUser,
    payload: Result<Json<RejectRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<ConfirmationDecision>>, AppError> {
    let Json(request) = parse_body(payload)?;
    let decision = state
        .confirmation_service
        .reject(&auth.user_id, request)
        .await?;
    Ok(Json(ApiResponse::success(decision)))
}

fn parse_body<T>(payload: Result<Json<T>, JsonRejection>) -> Result<Json<T>, AppError> {
    payload.map_err(|rejection| {
        AppError::invalid_param(format!("invalid request body: {}", rejection.body_text()))
    })
}
