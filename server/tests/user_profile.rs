//! 用户画像读写接口集成测试（REQ-202610-1176）。
//!
//! 覆盖：鉴权与用户归属校验、冷启动默认画像、显式写入、EWMA 刷新、
//! 更新频率控制，以及从 1174 / 1175 埋点表读取行为数据的联动。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::config::AppConfig;
use intimeui_server::db::Db;
use intimeui_server::state::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_config() -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        ..AppConfig::default()
    }
}

/// 测试库补齐依赖需求（1174 / 1175）的埋点表，模拟合并后的真实 schema。
async fn create_telemetry_tables(db: &Db) {
    let statements = [
        "CREATE TABLE IF NOT EXISTS telemetry_events (
            id TEXT PRIMARY KEY NOT NULL,
            event_id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            page_id TEXT NOT NULL,
            dwell_time INTEGER NOT NULL,
            element_click_rate REAL NOT NULL,
            misclick_rate REAL NOT NULL,
            scroll_depth REAL NOT NULL,
            occurred_at TEXT NOT NULL,
            received_at TEXT NOT NULL,
            UNIQUE (user_id, event_id)
        )",
        "CREATE TABLE IF NOT EXISTS telemetry_surveys (
            id TEXT PRIMARY KEY NOT NULL,
            survey_id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            session_id TEXT,
            scene_code TEXT NOT NULL,
            selected_style_id TEXT NOT NULL,
            rejected_style_id TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            received_at TEXT NOT NULL,
            CHECK (selected_style_id <> rejected_style_id),
            UNIQUE (user_id, survey_id)
        )",
    ];
    for statement in statements {
        sqlx::query(statement)
            .execute(db.pool())
            .await
            .expect("failed to create telemetry test table");
    }
}

async fn app_with_db(config: AppConfig) -> (axum::Router, Db) {
    let db = Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to run migrations");
    db.seed().await.expect("failed to load seed data");
    create_telemetry_tables(&db).await;
    let app = build_app(AppState::new(config, db.clone()));
    (app, db)
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

fn put_json(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
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

fn prompt_keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("prompt profile must be an object")
        .keys()
        .map(String::as_str)
        .collect()
}

#[tokio::test]
async fn get_profile_requires_authentication() {
    let (app, _db) = app_with_db(test_config()).await;
    let (status, body) = send(&app, get("/api/v1/users/usr_9527/profile")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
}

#[tokio::test]
async fn get_profile_returns_cold_start_defaults() {
    let (app, _db) = app_with_db(test_config()).await;
    let request = with_bearer(get("/api/v1/users/usr_9527/profile"), "dev:usr_9527");
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["user_id"], "usr_9527");
    assert_eq!(body["data"]["preferences"]["font_scale"], 1.0);
    assert_eq!(body["data"]["preferences"]["density"], 0.5);
    assert_eq!(body["data"]["preferences"]["theme"], "light");
    assert_eq!(body["data"]["preferences"]["radius"], 8.0);
    assert_eq!(body["data"]["preferences"]["spacing"], 1.0);
    assert_eq!(body["data"]["confidence"], 0.0);
    assert_eq!(body["data"]["confidence_level"], "cold_start");
    assert_eq!(body["data"]["cold_start"], true);
    assert_eq!(body["data"]["sample_count"], 0);
    assert_eq!(body["data"]["version"], 1);
    assert_eq!(body["data"]["updated_at"], Value::Null);
    assert_eq!(
        prompt_keys(&body["data"]["prompt_profile"]),
        vec![
            "cold_start",
            "confidence",
            "density",
            "font_scale",
            "radius",
            "spacing",
            "theme"
        ]
    );
    assert_eq!(body["data"]["prompt_profile"]["theme"], "light");
}

#[tokio::test]
async fn profile_scope_is_enforced() {
    let (app, _db) = app_with_db(test_config()).await;
    let request = with_bearer(get("/api/v1/users/usr_9527/profile"), "dev:someone_else");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], 40300);
}

#[tokio::test]
async fn put_profile_updates_preferences_and_prompt_json() {
    let (app, _db) = app_with_db(test_config()).await;
    let request = with_bearer(
        put_json(
            "/api/v1/users/usr_9527/profile",
            json!({ "font_scale": 1.1, "theme": "dark" }),
        ),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["preferences"]["font_scale"], 1.1);
    assert_eq!(body["data"]["preferences"]["theme"], "dark");
    assert_eq!(body["data"]["preferences"]["density"], 0.5);
    assert_eq!(body["data"]["version"], 2);
    assert_eq!(body["data"]["cold_start"], false);
    assert!(body["data"]["confidence"].as_f64().unwrap() >= 0.5);
    assert_eq!(body["data"]["prompt_profile"]["theme"], "dark");
    assert_eq!(body["data"]["prompt_profile"]["font_scale"], 1.1);
}

#[tokio::test]
async fn put_profile_rejects_invalid_values() {
    let (app, _db) = app_with_db(test_config()).await;

    let request = with_bearer(
        put_json(
            "/api/v1/users/usr_9527/profile",
            json!({ "font_scale": 2.0 }),
        ),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    // 空更新没有意义，直接拒绝。
    let request = with_bearer(
        put_json("/api/v1/users/usr_9527/profile", json!({})),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}

#[tokio::test]
async fn refresh_profile_applies_supplied_observations() {
    let (app, _db) = app_with_db(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/users/usr_9527/profile/refresh",
            json!({
                "force": true,
                "telemetry": [{
                    "dwell_time": 30000,
                    "element_click_rate": 0.8,
                    "misclick_rate": 0.02,
                    "scroll_depth": 0.9
                }],
                "surveys": [{
                    "selected_style_id": "style_dark_compact_large_font_rounded",
                    "rejected_style_id": "style_light_spacious_small_font_sharp"
                }]
            }),
        ),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["updated"], true);
    assert_eq!(body["data"]["observed_samples"], 2);
    assert_eq!(body["data"]["profile"]["sample_count"], 2);
    assert_eq!(body["data"]["profile"]["cold_start"], false);
    assert!(body["data"]["profile"]["confidence"].as_f64().unwrap() > 0.0);
    assert!(
        body["data"]["profile"]["preferences"]["font_scale"]
            .as_f64()
            .unwrap()
            > 1.0
    );
    assert!(
        body["data"]["profile"]["preferences"]["density"]
            .as_f64()
            .unwrap()
            < 0.5
    );
}

#[tokio::test]
async fn refresh_profile_rate_limits_rapid_calls() {
    let (app, _db) = app_with_db(test_config()).await;
    let payload = json!({
        "force": true,
        "telemetry": [{
            "dwell_time": 30000,
            "element_click_rate": 0.8,
            "misclick_rate": 0.02,
            "scroll_depth": 0.9
        }]
    });
    let first = with_bearer(
        post_json("/api/v1/users/usr_9527/profile/refresh", payload.clone()),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, first).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["updated"], true);

    let second_payload = json!({
        "telemetry": [{
            "dwell_time": 1000,
            "element_click_rate": 0.1,
            "misclick_rate": 0.5,
            "scroll_depth": 0.1
        }]
    });
    let second = with_bearer(
        post_json("/api/v1/users/usr_9527/profile/refresh", second_payload),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, second).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["updated"], false);
    assert_eq!(body["data"]["skipped_reason"], "rate_limited");
}

#[tokio::test]
async fn refresh_profile_without_data_reports_no_observations() {
    let (app, _db) = app_with_db(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/users/usr_9527/profile/refresh",
            json!({ "force": true }),
        ),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["updated"], false);
    assert_eq!(body["data"]["observed_samples"], 0);
    assert_eq!(body["data"]["skipped_reason"], "no_observations");
    assert_eq!(body["data"]["profile"]["cold_start"], true);
}

#[tokio::test]
async fn refresh_reads_telemetry_tables_when_no_observations_supplied() {
    let (app, db) = app_with_db(test_config()).await;
    let occurred_at = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO telemetry_events
             (id, event_id, user_id, page_id, dwell_time,
              element_click_rate, misclick_rate, scroll_depth, occurred_at, received_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("EVT_PROFILE_1")
    .bind("evt-profile-1")
    .bind("usr_9527")
    .bind("ORDER_DETAIL_PAGE")
    .bind(45_000_i64)
    .bind(0.7_f64)
    .bind(0.01_f64)
    .bind(0.9_f64)
    .bind(&occurred_at)
    .bind(&occurred_at)
    .execute(db.pool())
    .await
    .expect("insert telemetry event");

    sqlx::query(
        "INSERT INTO telemetry_surveys
             (id, survey_id, user_id, session_id, scene_code,
              selected_style_id, rejected_style_id, occurred_at, received_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("SURV_PROFILE_1")
    .bind("survey-profile-1")
    .bind("usr_9527")
    .bind("sess-profile-1")
    .bind("ORDER_DETAIL_PAGE")
    .bind("style_dark_compact_large_font")
    .bind("style_light_spacious_small_font")
    .bind(&occurred_at)
    .bind(&occurred_at)
    .execute(db.pool())
    .await
    .expect("insert telemetry survey");

    let request = with_bearer(
        post_json(
            "/api/v1/users/usr_9527/profile/refresh",
            json!({ "force": true, "window_hours": 24 }),
        ),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["updated"], true);
    assert_eq!(body["data"]["observed_samples"], 2);
    assert_eq!(body["data"]["profile"]["sample_count"], 2);
}
