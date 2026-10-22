//! 集成测试：显式偏好调研接口（REQ-202610-1175）。
//!
//! 覆盖：提交 / 幂等 / 查询 / 聚合 / 鉴权与用户隔离 / 字段校验。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use chrono::{Duration, Utc};
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

fn survey_body(
    scene_code: &str,
    selected_style_id: &str,
    rejected_style_id: &str,
    session_id: &str,
    survey_id: &str,
) -> Value {
    json!({
        "session_id": session_id,
        "scene_code": scene_code,
        "selected_style_id": selected_style_id,
        "rejected_style_id": rejected_style_id,
        "survey_id": survey_id
    })
}

fn survey_body_at(
    scene_code: &str,
    selected_style_id: &str,
    rejected_style_id: &str,
    session_id: &str,
    survey_id: &str,
    timestamp: chrono::DateTime<Utc>,
) -> Value {
    let mut body = survey_body(
        scene_code,
        selected_style_id,
        rejected_style_id,
        session_id,
        survey_id,
    );
    body["timestamp"] = json!(timestamp);
    body
}

async fn submit(app: &axum::Router, token: &str, body: Value) -> (StatusCode, Value) {
    send(
        app,
        with_bearer(post_json("/api/v1/telemetry/survey", body), token),
    )
    .await
}

#[tokio::test]
async fn submit_survey_persists_and_is_queryable() {
    let app = app().await;

    let (status, body) = submit(
        &app,
        "dev:u100",
        survey_body(
            "ORDER_DETAIL_PAGE",
            "style_modern_dark",
            "style_classic_light",
            "sess_001",
            "survey_001",
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["accepted"], true);
    assert_eq!(body["data"]["duplicate"], false);

    let survey = &body["data"]["survey"];
    let id = survey["id"].as_str().expect("survey id must be a string");
    assert!(id.starts_with("SURV_"));
    assert_eq!(survey["survey_id"], "survey_001");
    assert_eq!(survey["user_id"], "u100");
    assert_eq!(survey["session_id"], "sess_001");
    assert_eq!(survey["scene_code"], "ORDER_DETAIL_PAGE");
    assert_eq!(survey["selected_style_id"], "style_modern_dark");
    assert_eq!(survey["rejected_style_id"], "style_classic_light");
    assert!(survey["timestamp"].is_string());
    assert!(survey["received_at"].is_string());

    let (status, body) = send(
        &app,
        with_bearer(get("/api/v1/telemetry/surveys"), "dev:u100"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["limit"], 50);
    assert_eq!(body["data"]["offset"], 0);
    assert_eq!(body["data"]["items"][0]["id"], id);
    assert_eq!(body["data"]["items"][0]["scene_code"], "ORDER_DETAIL_PAGE");
}

#[tokio::test]
async fn submit_survey_requires_authentication() {
    let app = app().await;
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/telemetry/survey",
            survey_body("SCENE", "style_a", "style_b", "sess_1", "survey_1"),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
    assert_eq!(body["data"], Value::Null);
}

#[tokio::test]
async fn submit_survey_rejects_user_id_mismatch() {
    let app = app().await;
    let mut body = survey_body("SCENE", "style_a", "style_b", "sess_1", "survey_1");
    body["user_id"] = json!("u200");

    let (status, body) = submit(&app, "dev:u100", body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], 40300);
}

#[tokio::test]
async fn submit_survey_rejects_same_selected_and_rejected_style() {
    let app = app().await;
    let (status, body) = submit(
        &app,
        "dev:u100",
        survey_body("SCENE", "style_a", "style_a", "sess_1", "survey_1"),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert!(body["msg"]
        .as_str()
        .unwrap()
        .contains("selected_style_id and rejected_style_id must differ"));
}

#[tokio::test]
async fn submit_survey_rejects_invalid_identifiers_and_unknown_fields() {
    let app = app().await;

    // 样式标识不允许 HTML / 空格等字符。
    let (status, body) = submit(
        &app,
        "dev:u100",
        survey_body("SCENE", "<script>", "style_b", "sess_1", "survey_1"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    // 未知字段直接拒绝，避免把页面内容带进埋点库。
    let mut body = survey_body("SCENE", "style_a", "style_b", "sess_1", "survey_1");
    body["page_html"] = json!("<html></html>");
    let (status, body) = submit(&app, "dev:u100", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}

#[tokio::test]
async fn submit_survey_rejects_out_of_window_timestamps() {
    let app = app().await;

    let future = Utc::now() + Duration::minutes(10);
    let (status, body) = submit(
        &app,
        "dev:u100",
        survey_body_at("SCENE", "style_a", "style_b", "sess_1", "survey_1", future),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    let past = Utc::now() - Duration::days(91);
    let (status, body) = submit(
        &app,
        "dev:u100",
        survey_body_at("SCENE", "style_a", "style_b", "sess_1", "survey_2", past),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}

#[tokio::test]
async fn submit_survey_is_idempotent_by_survey_id() {
    let app = app().await;
    let body = survey_body("SCENE", "style_a", "style_b", "sess_1", "survey_dup");

    let (status, first) = submit(&app, "dev:u100", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["data"]["accepted"], true);
    assert_eq!(first["data"]["duplicate"], false);

    let (status, second) = submit(&app, "dev:u100", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["data"]["accepted"], false);
    assert_eq!(second["data"]["duplicate"], true);
    assert_eq!(
        second["data"]["survey"]["id"],
        first["data"]["survey"]["id"]
    );

    let (_, list) = send(
        &app,
        with_bearer(get("/api/v1/telemetry/surveys"), "dev:u100"),
    )
    .await;
    assert_eq!(list["data"]["total"], 1);
}

#[tokio::test]
async fn surveys_are_isolated_per_user() {
    let app = app().await;

    submit(
        &app,
        "dev:u100",
        survey_body("SCENE_A", "style_a", "style_b", "sess_1", "survey_1"),
    )
    .await;
    submit(
        &app,
        "dev:u200",
        survey_body("SCENE_B", "style_c", "style_d", "sess_2", "survey_2"),
    )
    .await;

    let (_, body) = send(
        &app,
        with_bearer(get("/api/v1/telemetry/surveys"), "dev:u100"),
    )
    .await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["user_id"], "u100");

    let (_, body) = send(
        &app,
        with_bearer(get("/api/v1/telemetry/surveys"), "dev:u200"),
    )
    .await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["user_id"], "u200");

    // 显式查询他人数据必须拒绝。
    let (status, body) = send(
        &app,
        with_bearer(get("/api/v1/telemetry/surveys?user_id=u200"), "dev:u100"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], 40300);
}

#[tokio::test]
async fn survey_list_supports_filters_and_pagination() {
    let app = app().await;
    let now = Utc::now();

    submit(
        &app,
        "dev:u100",
        survey_body_at(
            "ORDER_DETAIL_PAGE",
            "style_a",
            "style_b",
            "sess_a",
            "survey_1",
            now - Duration::hours(3),
        ),
    )
    .await;
    submit(
        &app,
        "dev:u100",
        survey_body_at(
            "ORDER_DETAIL_PAGE",
            "style_a",
            "style_c",
            "sess_b",
            "survey_2",
            now - Duration::hours(2),
        ),
    )
    .await;
    submit(
        &app,
        "dev:u100",
        survey_body_at(
            "ORDER_LIST_PAGE",
            "style_b",
            "style_a",
            "sess_a",
            "survey_3",
            now - Duration::hours(1),
        ),
    )
    .await;

    let (status, body) = send(
        &app,
        with_bearer(
            get("/api/v1/telemetry/surveys?scene_code=ORDER_DETAIL_PAGE"),
            "dev:u100",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 2);

    let (_, body) = send(
        &app,
        with_bearer(
            get("/api/v1/telemetry/surveys?session_id=sess_a"),
            "dev:u100",
        ),
    )
    .await;
    assert_eq!(body["data"]["total"], 2);

    // style_id 同时匹配 selected 与 rejected。
    let (_, body) = send(
        &app,
        with_bearer(
            get("/api/v1/telemetry/surveys?style_id=style_a"),
            "dev:u100",
        ),
    )
    .await;
    assert_eq!(body["data"]["total"], 3);

    // 使用 `Z` 后缀，避免 `+00:00` 在 query string 中被解码为空格。
    let from = (now - Duration::minutes(150)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let to = (now - Duration::minutes(90)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let path = format!("/api/v1/telemetry/surveys?from={from}&to={to}");
    let (_, body) = send(&app, with_bearer(get(&path), "dev:u100")).await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["survey_id"], "survey_2");

    let (_, body) = send(
        &app,
        with_bearer(
            get("/api/v1/telemetry/surveys?limit=1&offset=1"),
            "dev:u100",
        ),
    )
    .await;
    assert_eq!(body["data"]["total"], 3);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);

    let (status, body) = send(
        &app,
        with_bearer(get("/api/v1/telemetry/surveys?limit=0"), "dev:u100"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}

#[tokio::test]
async fn survey_aggregate_returns_style_scene_and_pair_stats() {
    let app = app().await;

    submit(
        &app,
        "dev:u100",
        survey_body("SCENE_X", "style_a", "style_b", "sess_1", "survey_1"),
    )
    .await;
    submit(
        &app,
        "dev:u100",
        survey_body("SCENE_X", "style_a", "style_b", "sess_2", "survey_2"),
    )
    .await;
    submit(
        &app,
        "dev:u100",
        survey_body("SCENE_Y", "style_b", "style_a", "sess_3", "survey_3"),
    )
    .await;

    let (status, body) = send(
        &app,
        with_bearer(get("/api/v1/telemetry/surveys/aggregate"), "dev:u100"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["total_surveys"], 3);

    let styles = body["data"]["styles"].as_array().unwrap();
    assert_eq!(styles.len(), 2);
    let style_a = styles
        .iter()
        .find(|style| style["style_id"] == "style_a")
        .unwrap();
    assert_eq!(style_a["selected_count"], 2);
    assert_eq!(style_a["rejected_count"], 1);
    assert_eq!(style_a["net_preference"], 1);

    let style_b = styles
        .iter()
        .find(|style| style["style_id"] == "style_b")
        .unwrap();
    assert_eq!(style_b["selected_count"], 1);
    assert_eq!(style_b["rejected_count"], 2);
    assert_eq!(style_b["net_preference"], -1);

    let scenes = body["data"]["scenes"].as_array().unwrap();
    let scene_x = scenes
        .iter()
        .find(|scene| scene["scene_code"] == "SCENE_X")
        .unwrap();
    assert_eq!(scene_x["survey_count"], 2);

    let pairs = body["data"]["pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0]["scene_code"], "SCENE_X");
    assert_eq!(pairs[0]["selected_style_id"], "style_a");
    assert_eq!(pairs[0]["rejected_style_id"], "style_b");
    assert_eq!(pairs[0]["count"], 2);

    // 场景过滤后只统计该场景样本。
    let (_, body) = send(
        &app,
        with_bearer(
            get("/api/v1/telemetry/surveys/aggregate?scene_code=SCENE_X"),
            "dev:u100",
        ),
    )
    .await;
    assert_eq!(body["data"]["total_surveys"], 2);
    assert_eq!(body["data"]["pairs"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["pairs"][0]["count"], 2);
}
