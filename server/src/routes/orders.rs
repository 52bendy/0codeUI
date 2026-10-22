//! 订单 / 用户余额查询端点（M0 数据底座对外示例）。
//!
//! 这些端点直接复用仓储层，返回结构与 PRD §5.1 `business_data` 对齐，
//! 后续 UI 生成接口（REQ-202610-1171）可直接复用同一份数据。

use axum::extract::{Path, State};
use axum::Json;

use crate::error::AppError;
use crate::models::{OrderBusinessData, UserBalance};
use crate::repository::Repository;
use crate::response::ApiResponse;
use crate::state::AppState;

/// `GET /api/v1/orders/{order_id}/business-data`
///
/// 返回 PRD §5.1 `business_data`：`{ order_id, status, items: [{ name, price, qty }] }`。
pub async fn order_business_data(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
) -> Result<Json<ApiResponse<OrderBusinessData>>, AppError> {
    let repository = Repository::from_db(&state.db);
    let data = repository.order_business_data(&order_id).await?;
    Ok(Json(ApiResponse::success(data)))
}

/// `GET /api/v1/users/{user_id}/balance`
///
/// 返回用户余额（`query_user_balance` 原子能力），金额为精确十进制数。
pub async fn user_balance(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<ApiResponse<UserBalance>>, AppError> {
    let repository = Repository::from_db(&state.db);
    let balance = repository.query_user_balance(&user_id).await?;
    Ok(Json(ApiResponse::success(balance)))
}
