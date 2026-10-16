//! 健康检查端点（基础可观测）。

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::response::ApiResponse;
use crate::state::AppState;

/// 健康检查响应数据。
#[derive(Debug, Serialize)]
pub struct HealthData {
    /// 固定为 `ok`（存活即健康；后续可扩展依赖探测）。
    pub status: &'static str,
    /// 服务名。
    pub service: &'static str,
    /// 版本号。
    pub version: &'static str,
    /// 运行环境。
    pub env: String,
    /// 已运行秒数。
    pub uptime_secs: u64,
}

/// `GET /healthz`、`GET /readyz`：存活 / 就绪检查（无需鉴权、不限流）。
pub async fn healthz(State(state): State<AppState>) -> Json<ApiResponse<HealthData>> {
    Json(ApiResponse::success(HealthData {
        status: "ok",
        service: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        env: state.config.env.as_str().to_string(),
        uptime_secs: state.uptime_secs(),
    }))
}
