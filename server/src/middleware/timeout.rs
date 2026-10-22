//! 请求超时中间件（超时统一返回 504）。

use std::time::Duration;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::state::AppState;

/// 对单个请求的完整处理耗时施加超时，超时返回统一 504 响应（code = 50400）。
pub async fn enforce_timeout(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let timeout = Duration::from_secs(state.config.request_timeout_secs.max(1));
    match tokio::time::timeout(timeout, next.run(req)).await {
        Ok(response) => response,
        Err(_) => {
            tracing::warn!(timeout_secs = timeout.as_secs(), "request timed out");
            AppError::timeout().into_response()
        }
    }
}
