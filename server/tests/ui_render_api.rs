//! 集成测试：`POST /api/v1/ui/render` 静态模板渲染接口（REQ-202610-1172）。

use std::time::Instant;

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

async fn app() -> axum::Router {
    let db = intimeui_server::db::Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to run migrations");
    db.seed().await.expect("failed to load seed data");
    build_app(AppState::new(test_config(), db))
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

fn order_detail_payload() -> Value {
    json!({
        "scene_code": "ORDER_DETAIL_PAGE",
        "business_data": {
            "order_id": "ORD_20261014_001",
            "status": "PAID",
            "items": [
                { "name": "智能降噪耳机", "price": 299.00, "qty": 1 }
            ]
        },
        "device_info": {
            "screen_width": 390,
            "screen_height": 844,
            "platform": "android"
        }
    })
}

#[tokio::test]
async fn ui_render_returns_static_template_html() {
    let app = app().await;
    let request = with_bearer(
        post_json("/api/v1/ui/render", order_detail_payload()),
        "dev:9527",
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["scene_code"], "ORDER_DETAIL_PAGE");
    assert_eq!(body["data"]["strategy"], "template");
    assert_eq!(body["data"]["template_version"], "v1");
    assert!(body["data"]["render_ms"].is_number());
    let html = body["data"]["html"].as_str().expect("html is string");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("ORD_20261014_001"));
    assert!(html.contains("智能降噪耳机"));
    assert!(html.contains("299.00"));
}

#[tokio::test]
async fn ui_render_inline_fetch_target_is_reachable() {
    let app = app().await;
    let request = with_bearer(
        post_json("/api/v1/ui/render", order_detail_payload()),
        "dev:9527",
    );
    let (_, body) = send(&app, request).await;
    let html = body["data"]["html"].as_str().expect("html is string");

    // 从内联 onclick 中取出 fetch 的目标地址，验证它确实能打到后端业务接口。
    let marker = "fetch('";
    let start = html.find(marker).expect("inline fetch binding exists") + marker.len();
    let rest = &html[start..];
    let end = rest.find('\'').expect("fetch url is quoted");
    let url = &rest[..end];

    assert_eq!(url, "/api/v1/orders/ORD_20261014_001/business-data");
    let (status, response) = send(&app, with_bearer(get(url), "dev:9527")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["order_id"], "ORD_20261014_001");
}

#[tokio::test]
async fn ui_render_requires_authentication() {
    let app = app().await;
    let (status, body) = send(&app, post_json("/api/v1/ui/render", order_detail_payload())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
}

#[tokio::test]
async fn ui_render_rejects_unknown_scene() {
    let app = app().await;
    let mut payload = order_detail_payload();
    payload["scene_code"] = json!("NOT_REGISTERED_PAGE");
    let request = with_bearer(post_json("/api/v1/ui/render", payload), "dev:9527");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], 40410);
}

#[tokio::test]
async fn ui_render_end_to_end_p95_is_under_200ms() {
    let app = app().await;
    let payload = order_detail_payload();

    for _ in 0..10 {
        let request = with_bearer(post_json("/api/v1/ui/render", payload.clone()), "dev:9527");
        let (status, _) = send(&app, request).await;
        assert_eq!(status, StatusCode::OK);
    }

    let mut samples = Vec::with_capacity(100);
    for _ in 0..100 {
        let started = Instant::now();
        let request = with_bearer(post_json("/api/v1/ui/render", payload.clone()), "dev:9527");
        let (status, _) = send(&app, request).await;
        samples.push(started.elapsed());
        assert_eq!(status, StatusCode::OK);
    }
    samples.sort_unstable();
    let p95 = samples[(samples.len() as f64 * 0.95).ceil() as usize - 1];
    println!("POST /api/v1/ui/render latency: p95={p95:?}");
    assert!(
        p95.as_millis() < 200,
        "end-to-end P95 must be < 200ms, got {p95:?}"
    );
}

#[tokio::test]
async fn ui_render_rejects_invalid_business_data() {
    let app = app().await;
    let mut payload = order_detail_payload();
    payload["business_data"]["items"] = json!([]);
    let request = with_bearer(post_json("/api/v1/ui/render", payload), "dev:9527");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}
