//! 路由注册。
//!
//! - 公开路由：`/`、`/healthz`、`/readyz`、`/api/v1/ping`；
//! - 受保护路由（需鉴权）：`/api/v1/me`、`/api/v1/echo`、
//!   `/api/v1/orders/{order_id}/business-data`、`/api/v1/users/{user_id}/balance`；
//! - 未匹配路径统一返回 404 响应结构。

pub mod example;
pub mod few_shot;
pub mod health;
pub mod orders;
pub mod profiles;

use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;

use crate::error::AppError;
use crate::middleware::{auth, rate_limit};
use crate::state::AppState;

/// 组装全部业务路由（含限流与鉴权）。
pub fn router(state: AppState) -> Router {
    let public_routes = Router::new()
        .route("/", get(example::root))
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/api/v1/ping", get(example::ping));

    let protected_routes = Router::new()
        .route("/api/v1/me", get(example::me))
        .route("/api/v1/echo", post(example::echo))
        .route(
            "/api/v1/orders/{order_id}/business-data",
            get(orders::order_business_data),
        )
        .route("/api/v1/users/{user_id}/balance", get(orders::user_balance))
        .route(
            "/api/v1/users/{user_id}/profile",
            get(profiles::get_profile).put(profiles::put_profile),
        )
        .route(
            "/api/v1/users/{user_id}/profile/refresh",
            post(profiles::refresh_profile),
        )
        .route(
            "/api/v1/few-shot/snippets",
            get(few_shot::list_snippets).post(few_shot::register_snippet),
        )
        .route(
            "/api/v1/few-shot/snippets/{id}/feedback",
            post(few_shot::snippet_feedback),
        )
        .route("/api/v1/few-shot/retrieve", post(few_shot::retrieve))
        .route_layer(from_fn_with_state(state.clone(), auth::require_auth));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .fallback(fallback_not_found)
        .layer(from_fn_with_state(
            state.clone(),
            rate_limit::limit_requests,
        ))
        .with_state(state)
}

/// 未匹配路由：统一 404 响应。
async fn fallback_not_found() -> AppError {
    AppError::not_found()
}
