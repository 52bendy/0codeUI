//! 集成测试：覆盖统一响应结构、鉴权、参数校验、限流与 404 兜底。

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::config::AppConfig;
use intimeui_server::state::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_config() -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        ..AppConfig::default()
    }
}

async fn app_with(config: AppConfig) -> axum::Router {
    let db = intimeui_server::db::Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to run migrations");
    db.seed().await.expect("failed to load seed data");
    build_app(AppState::new(config, db))
}

async fn send(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("request failed");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("failed to read body")
        .to_bytes();
    let value: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body is not valid JSON")
    };
    (status, value)
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn post_json(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn with_bearer(mut request: Request<Body>, token: &str) -> Request<Body> {
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    request
}

#[tokio::test]
async fn healthz_returns_unified_success() {
    let app = app_with(test_config()).await;
    let (status, body) = send(&app, get("/healthz")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["msg"], "ok");
    assert_eq!(body["data"]["status"], "ok");
    assert_eq!(body["data"]["service"], "intimeui-server");
    assert!(body["data"]["version"].is_string());
}

#[tokio::test]
async fn root_returns_service_info() {
    let app = app_with(test_config()).await;
    let (status, body) = send(&app, get("/")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["service"], "intimeui-server");
}

#[tokio::test]
async fn ping_is_public() {
    let app = app_with(test_config()).await;
    let (status, body) = send(&app, get("/api/v1/ping")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["pong"], Value::Bool(true));
}

#[tokio::test]
async fn unknown_route_returns_unified_404() {
    let app = app_with(test_config()).await;
    let (status, body) = send(&app, get("/definitely-not-here")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], 40400);
    assert_eq!(body["msg"], "not found");
    assert_eq!(body["data"], Value::Null);
}

#[tokio::test]
async fn echo_requires_authentication() {
    let app = app_with(test_config()).await;
    let (status, body) = send(&app, post_json("/api/v1/echo", json!({ "message": "hi" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
}

#[tokio::test]
async fn echo_rejects_invalid_payload() {
    let app = app_with(test_config()).await;

    // message 为空
    let request = with_bearer(
        post_json("/api/v1/echo", json!({ "message": "" })),
        "dev:1001",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    // priority 超范围
    let request = with_bearer(
        post_json("/api/v1/echo", json!({ "message": "hi", "priority": 42 })),
        "dev:1001",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    // 请求体不是合法 JSON
    let request = with_bearer(
        Request::builder()
            .method("POST")
            .uri("/api/v1/echo")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("not-json"))
            .unwrap(),
        "dev:1001",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}

#[tokio::test]
async fn echo_accepts_valid_request() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        post_json("/api/v1/echo", json!({ "message": "hello", "priority": 5 })),
        "dev:1001",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["user_id"], "1001");
    assert_eq!(body["data"]["message"], "hello");
    assert_eq!(body["data"]["priority"], 5);

    // priority 省略（None）也应通过
    let request = with_bearer(
        post_json("/api/v1/echo", json!({ "message": "no-priority" })),
        "dev:1001",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["priority"], Value::Null);
}

#[tokio::test]
async fn me_returns_authenticated_user() {
    let app = app_with(test_config()).await;
    let request = with_bearer(get("/api/v1/me"), "dev:2002");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["user_id"], "2002");
    assert_eq!(body["data"]["authenticated"], Value::Bool(true));
}

#[tokio::test]
async fn static_token_authentication() {
    let mut config = test_config();
    config.auth_dev_mode = false;
    config.auth_static_tokens = HashMap::from([("secret-token".to_string(), "u42".to_string())]);
    let app = app_with(config).await;

    // 静态令牌通过
    let request = with_bearer(get("/api/v1/me"), "secret-token");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["user_id"], "u42");

    // dev 令牌在关闭 dev 模式后无效
    let request = with_bearer(get("/api/v1/me"), "dev:1");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
}

#[tokio::test]
async fn rate_limit_rejects_after_burst() {
    let mut config = test_config();
    config.rate_limit_enabled = true;
    config.rate_limit_rps = 0.001;
    config.rate_limit_burst = 2;
    let app = app_with(config).await;

    let make_request = || with_bearer(get("/api/v1/ping"), "dev:rl");

    // 前两次（突发额度内）放行
    for _ in 0..2 {
        let (status, body) = send(&app, make_request()).await;
        assert_eq!(status, StatusCode::OK, "expected pass, body: {body}");
    }

    // 第三次触发限流
    let (status, body) = send(&app, make_request()).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], 42900);
    assert_eq!(body["msg"], "too many requests");
}

#[tokio::test]
async fn readyz_reports_database_ok() {
    let app = app_with(test_config()).await;
    let (status, body) = send(&app, get("/readyz")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["status"], "ok");
    assert_eq!(body["data"]["database"], "ok");
}

#[tokio::test]
async fn order_business_data_matches_prd_5_1() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        get("/api/v1/orders/ORD_20261014_001/business-data"),
        "dev:9527",
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["order_id"], "ORD_20261014_001");
    assert_eq!(body["data"]["status"], "PAID");

    let items = body["data"]["items"].as_array().expect("items is array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "智能降噪耳机");
    assert_eq!(items[0]["price"].to_string(), "299.00");
    assert_eq!(items[0]["qty"], 1);

    // 严格对齐 PRD §5.1：business_data 仅含 order_id / status / items，
    // items[] 仅含 name / price / qty。
    let data_keys: Vec<&str> = body["data"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(data_keys, vec!["items", "order_id", "status"]);
    let item_keys: Vec<&str> = items[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(item_keys, vec!["name", "price", "qty"]);
}

#[tokio::test]
async fn unknown_order_returns_unified_404() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        get("/api/v1/orders/ORD_DOES_NOT_EXIST/business-data"),
        "dev:9527",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], 40400);
    assert!(body["msg"]
        .as_str()
        .unwrap_or_default()
        .contains("order not found"));
}

#[tokio::test]
async fn user_balance_returns_exact_decimal() {
    let app = app_with(test_config()).await;
    let request = with_bearer(get("/api/v1/users/usr_9527/balance"), "dev:9527");
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["user_id"], "usr_9527");
    assert_eq!(body["data"]["balance"].to_string(), "1000.00");
    assert_eq!(body["data"]["currency"], "CNY");
}
