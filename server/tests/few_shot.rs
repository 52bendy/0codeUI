//! Few-Shot 模版检索集成测试（REQ-202610-1183）。
//!
//! 覆盖验收标准：
//! - 给定场景可检索出相关片段并注入 Prompt；
//! - 空库 / 低置信 / token 预算不足时安全降级为无 few-shot；
//! - 场景过滤、画像亲和度、反馈回流与输入安全校验。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::config::AppConfig;
use intimeui_server::db::Db;
use intimeui_server::state::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;

const USER_ID: &str = "usr_9527";

fn test_config() -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        ..AppConfig::default()
    }
}

async fn app_with(config: AppConfig) -> (axum::Router, Db) {
    let db = Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to run migrations");
    db.seed().await.expect("failed to load seed data");
    let app = build_app(AppState::new(config, db.clone()));
    (app, db)
}

/// 测试库补齐依赖需求（1174 / 1175）的埋点与调研表。
async fn create_signal_tables(db: &Db) {
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
            .expect("failed to create signal test table");
    }
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

fn put_json(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("PUT")
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

fn with_header(mut request: Request<Body>, name: &'static str, value: &str) -> Request<Body> {
    request.headers_mut().insert(name, value.parse().unwrap());
    request
}

fn retrieve_body(scene_code: &str, query: &str, base_prompt: Option<&str>) -> Value {
    let mut body = json!({
        "scene_code": scene_code,
        "query": query,
    });
    if let Some(base_prompt) = base_prompt {
        body["base_prompt"] = json!(base_prompt);
    }
    body
}

fn snippet_body(id: &str, scene_code: &str, satisfaction: f64) -> Value {
    json!({
        "id": id,
        "source_type": "CURATED",
        "source_ref": format!("test:{id}"),
        "scene_code": scene_code,
        "title": "测试订单卡片",
        "snippet_html": "<section class=\"order-card\"><h2>订单详情</h2><p>应付金额</p></section>",
        "tags": ["order", "test"],
        "style": { "theme": "any", "density": 0.5, "radius": 8.0 },
        "satisfaction": satisfaction
    })
}

#[tokio::test]
async fn retrieve_requires_authentication() {
    let (app, _db) = app_with(test_config()).await;
    let request = post_json(
        "/api/v1/few-shot/retrieve",
        retrieve_body("ORDER_DETAIL_PAGE", "订单", None),
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);
}

#[tokio::test]
async fn empty_library_degrades_safely() {
    let (app, _db) = app_with(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("EMPTY_SCENE", "订单", Some("BASE_PROMPT")),
        ),
        &format!("dev:{USER_ID}"),
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["degraded"], true);
    assert_eq!(body["data"]["degraded_reason"], "empty_library");
    assert_eq!(body["data"]["prompt_section"], "");
    assert_eq!(body["data"]["final_prompt"], "BASE_PROMPT");
    assert_eq!(body["data"]["snippets"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn disabled_feature_degrades_without_querying_library() {
    let config = AppConfig {
        rate_limit_enabled: false,
        few_shot_enabled: false,
        ..AppConfig::default()
    };
    let (app, _db) = app_with(config).await;
    let request = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("ORDER_DETAIL_PAGE", "订单", Some("BASE")),
        ),
        &format!("dev:{USER_ID}"),
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["degraded"], true);
    assert_eq!(body["data"]["degraded_reason"], "disabled");
    assert_eq!(body["data"]["final_prompt"], "BASE");
}

#[tokio::test]
async fn retrieves_scene_snippets_and_injects_prompt() {
    let (app, _db) = app_with(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("ORDER_DETAIL_PAGE", "订单详情 金额", Some("BASE_PROMPT")),
        ),
        &format!("dev:{USER_ID}"),
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["degraded"], false);
    assert!(data["degraded_reason"].is_null());
    assert!(data["candidate_count"].as_u64().unwrap() >= 2);

    let snippets = data["snippets"].as_array().unwrap();
    assert!(!snippets.is_empty());
    for snippet in snippets {
        assert_eq!(snippet["scene_code"], "ORDER_DETAIL_PAGE");
    }

    let section = data["prompt_section"].as_str().unwrap();
    assert!(section.contains("## Few-Shot 参考片段"));
    assert!(section.contains("```html"));
    assert!(section.contains("订单"));

    let final_prompt = data["final_prompt"].as_str().unwrap();
    assert!(final_prompt.starts_with("BASE_PROMPT"));
    assert!(final_prompt.contains(section));
}

#[tokio::test]
async fn scene_filter_excludes_other_scenes() {
    let (app, _db) = app_with(test_config()).await;
    let request = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("PRODUCT_DETAIL_PAGE", "商品 价格 库存", None),
        ),
        &format!("dev:{USER_ID}"),
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    let snippets = body["data"]["snippets"].as_array().unwrap();
    assert!(!snippets.is_empty());
    for snippet in snippets {
        assert_eq!(snippet["scene_code"], "PRODUCT_DETAIL_PAGE");
    }
}

#[tokio::test]
async fn profile_affinity_prefers_dark_compact_snippet() {
    let (app, _db) = app_with(test_config()).await;
    let token = format!("dev:{USER_ID}");

    let profile_request = with_bearer(
        put_json(
            &format!("/api/v1/users/{USER_ID}/profile"),
            json!({ "theme": "dark", "density": 0.2 }),
        ),
        &token,
    );
    let (status, _) = send(&app, profile_request).await;
    assert_eq!(status, StatusCode::OK);

    let request = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("ORDER_DETAIL_PAGE", "", None),
        ),
        &token,
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["profile_source"], "profile");
    let snippets = body["data"]["snippets"].as_array().unwrap();
    assert_eq!(snippets[0]["id"], "FSP_SEED_ORDER_DETAIL_DARK");
}

#[tokio::test]
async fn survey_signals_derive_profile_when_no_persisted_profile() {
    let (app, db) = app_with(test_config()).await;
    create_signal_tables(&db).await;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO telemetry_surveys
             (id, survey_id, user_id, session_id, scene_code,
              selected_style_id, rejected_style_id, occurred_at, received_at)
         VALUES ('survey_1', 'survey_1', ?, NULL, 'ORDER_DETAIL_PAGE',
                 'style_dark_compact', 'style_light_spacious', ?, ?)",
    )
    .bind(USER_ID)
    .bind(&now)
    .bind(&now)
    .execute(db.pool())
    .await
    .expect("failed to insert survey sample");

    let request = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("ORDER_DETAIL_PAGE", "", None),
        ),
        &format!("dev:{USER_ID}"),
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["profile_source"], "signals");
    let snippets = body["data"]["snippets"].as_array().unwrap();
    assert_eq!(snippets[0]["id"], "FSP_SEED_ORDER_DETAIL_DARK");
}

#[tokio::test]
async fn low_confidence_degrades_to_no_few_shot() {
    let config = AppConfig {
        rate_limit_enabled: false,
        few_shot_min_score: 0.99,
        ..AppConfig::default()
    };
    let (app, _db) = app_with(config).await;
    let request = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("ORDER_DETAIL_PAGE", "量子物理 天文观测", Some("BASE")),
        ),
        &format!("dev:{USER_ID}"),
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["degraded"], true);
    assert_eq!(body["data"]["degraded_reason"], "low_confidence");
    assert_eq!(body["data"]["prompt_section"], "");
    assert_eq!(body["data"]["final_prompt"], "BASE");
}

#[tokio::test]
async fn token_budget_exhausted_degrades_to_no_few_shot() {
    let config = AppConfig {
        rate_limit_enabled: false,
        few_shot_token_budget: 64,
        ..AppConfig::default()
    };
    let (app, _db) = app_with(config).await;
    let mut body = retrieve_body("ORDER_DETAIL_PAGE", "订单", Some("BASE"));
    body["token_budget"] = json!(64);
    let request = with_bearer(
        post_json("/api/v1/few-shot/retrieve", body),
        &format!("dev:{USER_ID}"),
    );
    let (status, response) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["data"]["degraded"], true);
    assert_eq!(response["data"]["degraded_reason"], "token_budget_exceeded");
    assert_eq!(response["data"]["prompt_section"], "");
    assert_eq!(response["data"]["final_prompt"], "BASE");
}

#[tokio::test]
async fn feedback_updates_satisfaction_and_usage() {
    let (app, _db) = app_with(test_config()).await;
    let token = format!("dev:{USER_ID}");

    let register = with_bearer(
        post_json(
            "/api/v1/few-shot/snippets",
            snippet_body("FSP_TEST_FEEDBACK", "FEEDBACK_SCENE", 0.5),
        ),
        &token,
    );
    let (status, _) = send(&app, register).await;
    assert_eq!(status, StatusCode::OK);

    let feedback = with_bearer(
        post_json(
            "/api/v1/few-shot/snippets/FSP_TEST_FEEDBACK/feedback",
            json!({ "satisfaction": 1.0 }),
        ),
        &token,
    );
    let (status, body) = send(&app, feedback).await;
    assert_eq!(status, StatusCode::OK);
    assert!((body["data"]["satisfaction"].as_f64().unwrap() - 0.65).abs() < 1e-9);
    assert_eq!(body["data"]["usage_count"], 1);
    assert_eq!(body["data"]["positive_feedback"], 1);

    let retrieve = with_bearer(
        post_json(
            "/api/v1/few-shot/retrieve",
            retrieve_body("FEEDBACK_SCENE", "", None),
        ),
        &token,
    );
    let (status, body) = send(&app, retrieve).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["snippets"][0]["id"], "FSP_TEST_FEEDBACK");
    assert!(
        (body["data"]["snippets"][0]["satisfaction"]
            .as_f64()
            .unwrap()
            - 0.65)
            .abs()
            < 1e-9
    );
}

#[tokio::test]
async fn rejects_script_in_snippet_html() {
    let (app, _db) = app_with(test_config()).await;
    let mut body = snippet_body("FSP_TEST_SCRIPT", "ORDER_DETAIL_PAGE", 0.9);
    body["snippet_html"] = json!("<div><script>alert(1)</script></div>");
    let request = with_bearer(
        post_json("/api/v1/few-shot/snippets", body),
        &format!("dev:{USER_ID}"),
    );
    let (status, response) = send(&app, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["code"], 40000);
}

#[tokio::test]
async fn rejects_inline_event_handler() {
    let (app, _db) = app_with(test_config()).await;
    let mut body = snippet_body("FSP_TEST_EVENT", "ORDER_DETAIL_PAGE", 0.9);
    body["snippet_html"] = json!("<div\nonclick=\"alert(1)\">x</div>");
    let request = with_bearer(
        post_json("/api/v1/few-shot/snippets", body),
        &format!("dev:{USER_ID}"),
    );
    let (status, response) = send(&app, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["code"], 40000);
}

#[tokio::test]
async fn rejects_invalid_scene_code() {
    let (app, _db) = app_with(test_config()).await;
    let body = snippet_body("FSP_TEST_SCENE", "订单详情", 0.9);
    let request = with_bearer(
        post_json("/api/v1/few-shot/snippets", body),
        &format!("dev:{USER_ID}"),
    );
    let (status, response) = send(&app, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["code"], 40000);
}

#[tokio::test]
async fn retrieve_rejects_other_user_id() {
    let (app, _db) = app_with(test_config()).await;
    let mut body = retrieve_body("ORDER_DETAIL_PAGE", "订单", None);
    body["user_id"] = json!("usr_other");
    let request = with_bearer(
        post_json("/api/v1/few-shot/retrieve", body),
        &format!("dev:{USER_ID}"),
    );
    let (status, response) = send(&app, request).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(response["code"], 40300);
}

#[tokio::test]
async fn admin_token_is_enforced_for_writes() {
    let config = AppConfig {
        rate_limit_enabled: false,
        few_shot_admin_token: Some("test-admin-token".to_string()),
        ..AppConfig::default()
    };
    let (app, _db) = app_with(config).await;
    let token = format!("dev:{USER_ID}");

    let without_header = with_bearer(
        post_json(
            "/api/v1/few-shot/snippets",
            snippet_body("FSP_TEST_ADMIN", "ORDER_DETAIL_PAGE", 0.9),
        ),
        &token,
    );
    let (status, response) = send(&app, without_header).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(response["code"], 40300);

    let with_token = with_bearer(
        with_header(
            post_json(
                "/api/v1/few-shot/snippets",
                snippet_body("FSP_TEST_ADMIN", "ORDER_DETAIL_PAGE", 0.9),
            ),
            "x-few-shot-token",
            "test-admin-token",
        ),
        &token,
    );
    let (status, response) = send(&app, with_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["data"]["id"], "FSP_TEST_ADMIN");
}

#[tokio::test]
async fn list_filters_by_scene_and_paginates() {
    let (app, _db) = app_with(test_config()).await;
    let request = with_bearer(
        get("/api/v1/few-shot/snippets?scene_code=ORDER_DETAIL_PAGE&limit=1&offset=0"),
        &format!("dev:{USER_ID}"),
    );
    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 2);
    assert_eq!(body["data"]["limit"], 1);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["items"][0]["scene_code"], "ORDER_DETAIL_PAGE");
}
