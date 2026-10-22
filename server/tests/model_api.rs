//! 模型推理接口集成测试：鉴权、生成、校验、指标与 SSE。

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::config::AppConfig;
use intimeui_server::model::ModelConfig;
use intimeui_server::state::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_config() -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        model: ModelConfig {
            mock_latency_ms: 0,
            mock_stream_delay_ms: 0,
            ..ModelConfig::default()
        },
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

fn post_json(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn with_bearer(mut request: Request<Body>, token: &str) -> Request<Body> {
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    request
}

async fn send_json(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("request failed");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("failed to read body")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body is not valid JSON")
    };
    (status, value)
}

#[tokio::test]
async fn generate_requires_authentication() {
    let app = app_with(test_config()).await;
    let request = post_json(
        "/api/v1/ui/model/generate",
        json!({ "prompt": "生成一个登录页" }),
    );
    let (status, body) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
}

#[tokio::test]
async fn generate_returns_valid_single_file_html() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/ui/model/generate",
            json!({ "prompt": "生成一个订单详情页，黑白灰主题" }),
        ),
        "dev:1001",
    );
    let (status, body) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    let html = body["data"]["html"].as_str().expect("html is missing");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<html"));
    assert!(html.contains("<body"));
    assert_eq!(body["data"]["model"], "Qwen2.5-Coder-7B-Instruct-AWQ");
    assert!(body["data"]["latency_ms"].as_f64().unwrap() >= 0.0);
}

#[tokio::test]
async fn generate_rejects_empty_prompt() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        post_json("/api/v1/ui/model/generate", json!({ "prompt": "" })),
        "dev:1001",
    );
    let (status, body) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}

#[tokio::test]
async fn generate_rejects_unknown_model() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/ui/model/generate",
            json!({ "prompt": "生成一个登录页", "model": "not-registered" }),
        ),
        "dev:1001",
    );
    let (status, body) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert!(body["msg"].as_str().unwrap().contains("unknown model"));
}

#[tokio::test]
async fn generate_rejects_out_of_range_parameters() {
    let app = app_with(test_config()).await;
    for payload in [
        json!({ "prompt": "生成页面", "max_tokens": 0 }),
        json!({ "prompt": "生成页面", "max_tokens": 9000 }),
        json!({ "prompt": "生成页面", "temperature": 3.0 }),
    ] {
        let request = with_bearer(post_json("/api/v1/ui/model/generate", payload), "dev:1001");
        let (status, body) = send_json(&app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], 40000);
    }
}

#[tokio::test]
async fn metrics_reflect_generate_calls() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        post_json("/api/v1/ui/model/generate", json!({ "prompt": "生成页面" })),
        "dev:1001",
    );
    let (status, _) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::OK);

    let request = with_bearer(get("/api/v1/ui/model/metrics"), "dev:1001");
    let (status, body) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["total_requests"], 1);
    assert_eq!(body["data"]["success_requests"], 1);
    assert!(body["data"]["p95_latency_ms"].as_f64().unwrap() >= 0.0);
    assert_eq!(body["data"]["by_model"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn info_exposes_provider_and_models() {
    let app = app_with(test_config()).await;
    let request = with_bearer(get("/api/v1/ui/model/info"), "dev:1001");
    let (status, body) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["provider"], "mock");
    assert_eq!(body["data"]["enabled"], true);
    assert!(body["data"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model == "Qwen2.5-Coder-7B-Instruct-AWQ"));
}

#[tokio::test]
async fn stream_emits_meta_delta_and_done_events() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/ui/model/stream",
            json!({ "prompt": "生成一个个人中心页" }),
        ),
        "dev:1001",
    );
    let response = app.clone().oneshot(request).await.expect("request failed");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .starts_with("text/event-stream"));
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read SSE body");
    let body = String::from_utf8(bytes.to_vec()).expect("SSE body is not UTF-8");
    assert!(body.contains("event: meta"));
    assert!(body.contains("event: delta"));
    assert!(body.contains("event: done"));
    assert!(body.contains("\"model\""));
}

#[tokio::test]
async fn disabled_model_service_returns_503() {
    let mut config = test_config();
    config.model.enabled = false;
    let app = app_with(config).await;
    let request = with_bearer(
        post_json("/api/v1/ui/model/generate", json!({ "prompt": "生成页面" })),
        "dev:1001",
    );
    let (status, body) = send_json(&app, request).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], 50300);
}
