//! 路由注册。
//!
//! - 公开路由：`/`、`/healthz`、`/readyz`、`/api/v1/ping`；
//! - 受保护路由（需鉴权）：`/api/v1/me`、`/api/v1/echo`、
//!   `/api/v1/orders/{order_id}/business-data`、`/api/v1/users/{user_id}/balance`、
//!   `/api/v1/telemetry/survey`、`/api/v1/telemetry/surveys`、
//!   `/api/v1/telemetry/surveys/aggregate`、`/api/v1/ui/generate`、
//!   `/api/v1/ui/render`、`/api/v1/ui/obfuscate`、`/api/v1/preference/*`；
//! - 未匹配路径统一返回 404 响应结构。

pub mod example;
pub mod health;
pub mod model;
pub mod orders;
pub mod pipeline;
pub mod preference;
pub mod profiles;
pub mod telemetry_survey;
pub mod ui;
pub mod ui_obfuscate;
pub mod ui_render;

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
            "/api/v1/telemetry/survey",
            post(telemetry_survey::submit_survey),
        )
        .route(
            "/api/v1/telemetry/surveys",
            get(telemetry_survey::list_surveys),
        )
        .route(
            "/api/v1/telemetry/surveys/aggregate",
            get(telemetry_survey::aggregate_surveys),
        )
        .route("/api/v1/ui/generate", post(ui::generate))
        .route(
            "/api/v1/ui/generate/dynamic",
            post(pipeline::generate_dynamic),
        )
        .route(
            "/api/v1/ui/generate/dynamic/stream",
            post(pipeline::generate_dynamic_stream),
        )
        .route("/api/v1/ui/render", post(ui_render::render_ui))
        .route("/api/v1/ui/obfuscate", post(ui_obfuscate::obfuscate))
        .route("/api/v1/ui/pipeline/metrics", get(pipeline::metrics))
        .route("/api/v1/ui/pipeline/info", get(pipeline::info))
        .route("/api/v1/ui/model/generate", post(model::generate))
        .route("/api/v1/ui/model/stream", post(model::stream))
        .route("/api/v1/ui/model/info", get(model::info))
        .route("/api/v1/ui/model/metrics", get(model::metrics))
        .route(
            "/api/v1/users/{user_id}/profile",
            get(profiles::get_profile).put(profiles::put_profile),
        )
        .route(
            "/api/v1/users/{user_id}/profile/refresh",
            post(profiles::refresh_profile),
        )
        .route(
            "/api/v1/preference/pairs",
            post(preference::ingest_pair).get(preference::list_pairs),
        )
        .route(
            "/api/v1/preference/pairs/from-survey",
            post(preference::ingest_from_survey),
        )
        .route("/api/v1/preference/pairs/{id}", get(preference::get_pair))
        .route(
            "/api/v1/preference/pairs/{id}/review",
            post(preference::review_pair),
        )
        .route("/api/v1/preference/stats", get(preference::stats))
        .route("/api/v1/preference/sample", get(preference::sample))
        .route("/api/v1/preference/export", post(preference::export))
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
