//! 集成测试：UI 生成接口契约（REQ-202610-1171）。
//!
//! 覆盖：成功响应结构、单文件 HTML、未知场景错误码、鉴权、
//! 请求字段校验、业务数据校验与用户归属。

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
        auth_dev_mode: true,
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

fn order_detail_request() -> Value {
    json!({
        "user_id": "usr_9527",
        "scene_code": "ORDER_DETAIL_PAGE",
        "device_info": {
            "screen_width": 390,
            "screen_height": 844,
            "platform": "android"
        },
        "business_data": {
            "order_id": "ORD_20261014_001",
            "status": "PAID",
            "items": [
                { "name": "智能降噪耳机", "price": 299.00, "qty": 1 }
            ]
        }
    })
}

#[tokio::test]
async fn order_detail_returns_complete_single_file_html() {
    let app = app_with(test_config()).await;
    let request = with_bearer(
        post_json("/api/v1/ui/generate", order_detail_request()),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["msg"], "ok");

    let data = body["data"].as_object().expect("data must be an object");
    assert_eq!(data.len(), 3, "response data must expose exactly 3 fields");
    assert_eq!(data["render_type"], "SINGLE_FILE_HTML");
    assert_eq!(data["version"], "template:ORDER_DETAIL_PAGE:v1");

    let html = data["html_content"]
        .as_str()
        .expect("html_content must be a string");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.trim_end().ends_with("</html>"));
    assert!(html.contains("ORD_20261014_001"));
    assert!(html.contains("智能降噪耳机"));
    assert!(html.contains("Content-Security-Policy"));
    assert!(!html.contains("http://"));
    assert!(!html.contains("https://"));
}

#[tokio::test]
async fn order_list_scene_returns_expected_version() {
    let app = app_with(test_config()).await;
    let payload = json!({
        "user_id": "usr_9527",
        "scene_code": "ORDER_LIST_PAGE",
        "device_info": { "screen_width": 390, "screen_height": 844, "platform": "ios" },
        "business_data": {
            "orders": [
                { "order_id": "ORD_1", "status": "PAID", "total_amount": 299.00, "item_count": 1 }
            ]
        }
    });
    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["version"], "template:ORDER_LIST_PAGE:v1");
    assert!(body["data"]["html_content"]
        .as_str()
        .unwrap()
        .contains("ORD_1"));
}

#[tokio::test]
async fn product_detail_scene_returns_expected_version() {
    let app = app_with(test_config()).await;
    let payload = json!({
        "user_id": "usr_9527",
        "scene_code": "PRODUCT_DETAIL_PAGE",
        "device_info": { "screen_width": 1280, "screen_height": 800, "platform": "web" },
        "business_data": {
            "product_id": "PRD_1",
            "name": "智能降噪耳机",
            "price": 299.00,
            "stock_status": "IN_STOCK"
        }
    });
    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["version"], "template:PRODUCT_DETAIL_PAGE:v1");
    assert!(body["data"]["html_content"]
        .as_str()
        .unwrap()
        .contains("智能降噪耳机"));
}

#[tokio::test]
async fn unknown_scene_returns_40410_with_available_scenes() {
    let app = app_with(test_config()).await;
    let mut payload = order_detail_request();
    payload["scene_code"] = json!("NOT_A_SCENE");

    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], 40410);
    assert_eq!(body["data"], Value::Null);
    let message = body["msg"].as_str().unwrap();
    assert!(message.contains("unknown scene_code"));
    assert!(message.contains("ORDER_DETAIL_PAGE"));
}

#[tokio::test]
async fn generate_requires_authentication() {
    let app = app_with(test_config()).await;
    let (status, body) = send(
        &app,
        post_json("/api/v1/ui/generate", order_detail_request()),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
    assert_eq!(body["data"], Value::Null);
}

#[tokio::test]
async fn missing_required_field_returns_invalid_param() {
    let app = app_with(test_config()).await;
    let mut payload = order_detail_request();
    payload.as_object_mut().unwrap().remove("device_info");

    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert!(body["msg"]
        .as_str()
        .unwrap()
        .contains("invalid request body"));
}

#[tokio::test]
async fn invalid_business_data_returns_invalid_param() {
    let app = app_with(test_config()).await;
    let mut payload = order_detail_request();
    payload["business_data"]["items"][0]["price"] = json!(-1.00);

    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert!(body["msg"].as_str().unwrap().contains("price"));
}

#[tokio::test]
async fn non_object_business_data_returns_invalid_param() {
    let app = app_with(test_config()).await;
    let mut payload = order_detail_request();
    payload["business_data"] = json!([1, 2, 3]);

    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert!(body["msg"].as_str().unwrap().contains("JSON object"));
}

#[tokio::test]
async fn invalid_device_platform_returns_invalid_param() {
    let app = app_with(test_config()).await;
    let mut payload = order_detail_request();
    payload["device_info"]["platform"] = json!("blackberry");

    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert!(body["msg"].as_str().unwrap().contains("platform"));
}

#[tokio::test]
async fn production_rejects_user_id_mismatch() {
    let config = AppConfig {
        rate_limit_enabled: false,
        auth_dev_mode: false,
        auth_static_tokens: [("tok-1".to_string(), "usr_1".to_string())]
            .into_iter()
            .collect(),
        ..AppConfig::default()
    };
    let app = app_with(config).await;
    let mut payload = order_detail_request();
    payload["user_id"] = json!("usr_2");

    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "tok-1"),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], 40300);
    assert!(body["msg"]
        .as_str()
        .unwrap()
        .contains("does not match authenticated user"));
}

#[tokio::test]
async fn invalid_screen_width_returns_invalid_param() {
    let app = app_with(test_config()).await;
    let mut payload = order_detail_request();
    payload["device_info"]["screen_width"] = json!(0);

    let (status, body) = send(
        &app,
        with_bearer(post_json("/api/v1/ui/generate", payload), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert!(body["msg"].as_str().unwrap().contains("screen_width"));
}
