//! 示例端点：演示统一响应结构、鉴权（user_id）与参数校验的组合用法。

use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::middleware::auth::AuthUser;
use crate::response::ApiResponse;
use crate::validation::ValidatedJson;

/// `GET /`：服务自述。
#[derive(Debug, Serialize)]
pub struct ServiceInfo {
    pub service: &'static str,
    pub version: &'static str,
    pub message: &'static str,
}

pub async fn root() -> Json<ApiResponse<ServiceInfo>> {
    Json(ApiResponse::success(ServiceInfo {
        service: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        message: "InTimeUI backend service",
    }))
}

/// `GET /api/v1/ping`：公开探活示例。
#[derive(Debug, Serialize)]
pub struct PingData {
    pub pong: bool,
    pub ts_ms: u128,
}

pub async fn ping() -> Json<ApiResponse<PingData>> {
    Json(ApiResponse::success(PingData {
        pong: true,
        ts_ms: now_ms(),
    }))
}

/// `GET /api/v1/me`：返回当前登录用户标识（鉴权示例）。
#[derive(Debug, Serialize)]
pub struct MeData {
    pub user_id: String,
    pub authenticated: bool,
}

pub async fn me(user: AuthUser) -> Json<ApiResponse<MeData>> {
    Json(ApiResponse::success(MeData {
        user_id: user.user_id,
        authenticated: true,
    }))
}

/// `POST /api/v1/echo`：鉴权 + 参数校验示例。
#[derive(Debug, Deserialize, Validate)]
pub struct EchoRequest {
    /// 回显内容，1~500 字符。
    #[validate(length(min = 1, max = 500, message = "message must be 1..=500 characters"))]
    pub message: String,
    /// 可选优先级，1~9。
    #[validate(range(min = 1, max = 9, message = "priority must be within 1..=9"))]
    pub priority: Option<u8>,
}

#[derive(Debug, Serialize)]
pub struct EchoData {
    pub user_id: String,
    pub message: String,
    pub priority: Option<u8>,
    pub received_at_ms: u128,
}

pub async fn echo(
    user: AuthUser,
    ValidatedJson(payload): ValidatedJson<EchoRequest>,
) -> Json<ApiResponse<EchoData>> {
    Json(ApiResponse::success(EchoData {
        user_id: user.user_id,
        message: payload.message,
        priority: payload.priority,
        received_at_ms: now_ms(),
    }))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
