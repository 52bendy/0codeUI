//! 集成测试：偏好对齐数据管道（REQ-202610-1188）。
//!
//! 覆盖：入库 / 幂等 / 清洗 / PII 脱敏 / 调研归集 / 审核 / 统计 / 抽样 /
//! 导出（用户哈希、配额、清单）/ 管理令牌 / 鉴权与参数校验。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::config::AppConfig;
use intimeui_server::preference::config::PreferenceConfig;
use intimeui_server::state::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;

const CHOSEN_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>订单</title><style>body{margin:0}</style></head>
<body><main><h1>订单详情</h1><p>金额 299 元</p></main></body>
</html>"#;

const REJECTED_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>订单</title><style>body{margin:8px}</style></head>
<body><main><h1>订单</h1><p>299</p></main></body>
</html>"#;

const PROMPT: &str = "生成一个订单详情页面，展示商品、金额与状态。";

fn test_config() -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        ..AppConfig::default()
    }
}

fn config_with_admin_token(token: &str) -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        preference: PreferenceConfig {
            admin_token: Some(token.to_string()),
            ..PreferenceConfig::default()
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

async fn app() -> axum::Router {
    app_with(test_config()).await
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

fn with_admin(mut request: Request<Body>, token: &str) -> Request<Body> {
    request
        .headers_mut()
        .insert("x-preference-token", token.parse().unwrap());
    request
}

fn pair_body(scene_code: &str, prompt: &str) -> Value {
    json!({
        "prompt": prompt,
        "chosen_html": CHOSEN_HTML,
        "rejected_html": REJECTED_HTML,
        "source": "AB_TEST",
        "scene_code": scene_code,
        "session_id": "sess_001",
        "request_id": "req_001",
        "prompt_template_version": "prompt-v1",
        "chosen_model": "qwen2.5-coder-7b",
        "rejected_model": "qwen2.5-coder-7b",
        "chosen_style_id": "style_a",
        "rejected_style_id": "style_b",
        "pipeline_version": "pipeline-v1",
        "preference_strength": 1.0,
        "metadata": {"experiment": "exp-001"}
    })
}

async fn ingest(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    send(
        app,
        with_bearer(post_json("/api/v1/preference/pairs", body), "dev:u100"),
    )
    .await
}

async fn ingest_pair(app: &axum::Router, scene_code: &str, prompt: &str) -> String {
    let (status, body) = ingest(app, pair_body(scene_code, prompt)).await;
    assert_eq!(status, StatusCode::OK, "ingest failed: {body}");
    body["data"]["pair"]["id"]
        .as_str()
        .expect("pair id must be a string")
        .to_string()
}

async fn submit_survey(app: &axum::Router, user: &str, survey_id: &str) -> (StatusCode, Value) {
    send(
        app,
        with_bearer(
            post_json(
                "/api/v1/telemetry/survey",
                json!({
                    "session_id": "sess_survey",
                    "scene_code": "ORDER_DETAIL_PAGE",
                    "selected_style_id": "style_modern",
                    "rejected_style_id": "style_classic",
                    "survey_id": survey_id
                }),
            ),
            user,
        ),
    )
    .await
}

#[tokio::test]
async fn ingest_persists_pair_with_provenance_and_auto_accepts() {
    let app = app().await;
    let (status, body) = ingest(&app, pair_body("ORDER_DETAIL_PAGE", PROMPT)).await;

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["accepted"], true);
    assert_eq!(body["data"]["duplicate"], false);

    let pair = &body["data"]["pair"];
    assert!(pair["id"].as_str().unwrap().starts_with("PREF_"));
    assert_eq!(pair["status"], "ACCEPTED");
    assert_eq!(pair["quality_score"], 1.0);
    assert_eq!(pair["source"], "AB_TEST");
    assert_eq!(pair["scene_code"], "ORDER_DETAIL_PAGE");
    assert_eq!(pair["user_id"], "u100");
    assert_eq!(pair["prompt_template_version"], "prompt-v1");
    assert_eq!(pair["chosen_model"], "qwen2.5-coder-7b");
    assert_eq!(pair["pipeline_version"], "pipeline-v1");
    assert_eq!(pair["chosen_style_id"], "style_a");
    assert_eq!(pair["rejected_style_id"], "style_b");
    assert_eq!(pair["metadata"]["experiment"], "exp-001");
    assert!(pair["pair_key"].as_str().unwrap().len() == 64);
    assert!(pair["content_hash"].as_str().unwrap().len() == 64);
}

#[tokio::test]
async fn duplicate_pair_is_idempotent() {
    let app = app().await;
    let body = pair_body("ORDER_DETAIL_PAGE", PROMPT);

    let (status, first) = ingest(&app, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, second) = ingest(&app, body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["data"]["accepted"], false);
    assert_eq!(second["data"]["duplicate"], true);
    assert_eq!(second["data"]["pair"]["id"], first["data"]["pair"]["id"]);

    let (_, list) = send(
        &app,
        with_bearer(get("/api/v1/preference/pairs"), "dev:u100"),
    )
    .await;
    assert_eq!(list["data"]["total"], 1);
}

#[tokio::test]
async fn identical_outputs_are_rejected() {
    let app = app().await;
    let mut body = pair_body("ORDER_DETAIL_PAGE", PROMPT);
    body["rejected_html"] = body["chosen_html"].clone();

    let (status, response) = ingest(&app, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["code"], 40000);
    assert!(response["msg"]
        .as_str()
        .unwrap()
        .contains("chosen_html and rejected_html must differ"));
}

#[tokio::test]
async fn invalid_chosen_html_is_rejected() {
    let app = app().await;
    let mut body = pair_body("ORDER_DETAIL_PAGE", PROMPT);
    body["chosen_html"] = json!("<html><body>broken");

    let (status, response) = ingest(&app, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["code"], 40000);
    assert!(response["msg"]
        .as_str()
        .unwrap()
        .contains("chosen_html is not valid single-file HTML"));
}

#[tokio::test]
async fn invalid_rejected_html_is_flagged_and_goes_to_review() {
    let app = app().await;
    let mut body = pair_body("ORDER_DETAIL_PAGE", PROMPT);
    body["rejected_html"] = json!("<html><body>broken");

    let (status, response) = ingest(&app, body).await;
    assert_eq!(status, StatusCode::OK, "body: {response}");
    let pair = &response["data"]["pair"];
    assert_eq!(pair["status"], "PENDING");
    assert!(pair["quality_flags"]
        .as_array()
        .unwrap()
        .contains(&json!("rejected_html_invalid")));
    assert!(pair["quality_score"].as_f64().unwrap() < 1.0);
}

#[tokio::test]
async fn pii_is_redacted_in_prompt_and_metadata() {
    let app = app().await;
    let mut body = pair_body(
        "ORDER_DETAIL_PAGE",
        "联系 test@example.com 或 13800138000 生成订单页",
    );
    body["metadata"] = json!({ "note": "备用 13900139000" });

    let (status, response) = ingest(&app, body).await;
    assert_eq!(status, StatusCode::OK, "body: {response}");
    let pair = &response["data"]["pair"];
    assert!(pair["prompt"].as_str().unwrap().contains("[EMAIL]"));
    assert!(pair["prompt"].as_str().unwrap().contains("[PHONE]"));
    assert!(!pair["prompt"].as_str().unwrap().contains("13800138000"));
    assert_eq!(pair["metadata"]["note"], "备用 [PHONE]");
    assert!(pair["quality_flags"]
        .as_array()
        .unwrap()
        .contains(&json!("pii_redacted")));
}

#[tokio::test]
async fn from_survey_reads_provenance_from_survey_record() {
    let app = app().await;
    let (status, survey) = submit_survey(&app, "dev:u100", "survey_001").await;
    assert_eq!(status, StatusCode::OK, "survey: {survey}");

    let body = json!({
        "survey_id": "survey_001",
        "prompt": PROMPT,
        "chosen_html": CHOSEN_HTML,
        "rejected_html": REJECTED_HTML,
        "prompt_template_version": "prompt-v1",
        "chosen_model": "qwen2.5-coder-7b",
        "rejected_model": "qwen2.5-coder-7b",
        "pipeline_version": "pipeline-v1",
        "preference_strength": 1.0
    });
    let (status, response) = send(
        &app,
        with_bearer(
            post_json("/api/v1/preference/pairs/from-survey", body),
            "dev:u100",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {response}");
    let pair = &response["data"]["pair"];
    assert_eq!(pair["source"], "EXPLICIT_SURVEY");
    assert_eq!(pair["scene_code"], "ORDER_DETAIL_PAGE");
    assert_eq!(pair["session_id"], "sess_survey");
    assert_eq!(pair["chosen_style_id"], "style_modern");
    assert_eq!(pair["rejected_style_id"], "style_classic");
    assert_eq!(pair["survey_id"], "survey_001");
    assert_eq!(pair["status"], "ACCEPTED");
}

#[tokio::test]
async fn from_survey_rejects_unknown_survey() {
    let app = app().await;
    let body = json!({
        "survey_id": "survey_missing",
        "prompt": PROMPT,
        "chosen_html": CHOSEN_HTML,
        "rejected_html": REJECTED_HTML,
        "prompt_template_version": "prompt-v1",
        "chosen_model": "qwen2.5-coder-7b"
    });
    let (status, response) = send(
        &app,
        with_bearer(
            post_json("/api/v1/preference/pairs/from-survey", body),
            "dev:u100",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(response["code"], 40400);
}

#[tokio::test]
async fn review_accepts_pending_pair_and_export_marks_it_exported() {
    let app = app().await;
    let mut body = pair_body("ORDER_DETAIL_PAGE", PROMPT);
    body["rejected_html"] = json!("<html><body>broken");
    let (status, created) = ingest(&app, body).await;
    assert_eq!(status, StatusCode::OK);
    let id = created["data"]["pair"]["id"].as_str().unwrap();

    let (status, reviewed) = send(
        &app,
        with_bearer(
            post_json(
                &format!("/api/v1/preference/pairs/{id}/review"),
                json!({"decision": "ACCEPT", "note": "人工确认可训练"}),
            ),
            "dev:admin",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "review: {reviewed}");
    assert_eq!(reviewed["data"]["status"], "ACCEPTED");
    assert_eq!(reviewed["data"]["reviewed_by"], "admin");

    let (status, exported) = send(
        &app,
        with_bearer(
            post_json("/api/v1/preference/export", json!({})),
            "dev:admin",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "export: {exported}");
    assert_eq!(exported["data"]["manifest"]["record_count"], 1);
    assert_eq!(exported["data"]["records"][0]["id"], id);

    let (status, fetched) = send(
        &app,
        with_bearer(get(&format!("/api/v1/preference/pairs/{id}")), "dev:admin"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["data"]["status"], "EXPORTED");
}

#[tokio::test]
async fn rejected_pair_is_excluded_from_export() {
    let app = app().await;
    let mut body = pair_body("ORDER_DETAIL_PAGE", PROMPT);
    body["rejected_html"] = json!("<html><body>broken");
    let (_, created) = ingest(&app, body).await;
    let id = created["data"]["pair"]["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        with_bearer(
            post_json(
                &format!("/api/v1/preference/pairs/{id}/review"),
                json!({"decision": "REJECT", "note": "质量不足"}),
            ),
            "dev:admin",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, exported) = send(
        &app,
        with_bearer(
            post_json("/api/v1/preference/export", json!({})),
            "dev:admin",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(exported["data"]["manifest"]["record_count"], 0);
}

#[tokio::test]
async fn stats_counts_status_and_source() {
    let app = app().await;
    ingest_pair(&app, "ORDER_DETAIL_PAGE", PROMPT).await;
    let mut pending = pair_body("CART_PAGE", "生成购物车页面，展示商品数量与总价。");
    pending["rejected_html"] = json!("<html><body>broken");
    let (status, _) = ingest(&app, pending).await;
    assert_eq!(status, StatusCode::OK);

    let (status, stats) = send(
        &app,
        with_bearer(get("/api/v1/preference/stats"), "dev:admin"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stats: {stats}");
    assert_eq!(stats["data"]["total"], 2);
    assert_eq!(stats["data"]["exportable"], 1);
    assert_eq!(stats["data"]["by_status"][0]["count"], 1);
    assert_eq!(stats["data"]["by_source"][0]["key"], "AB_TEST");
}

#[tokio::test]
async fn sample_is_deterministic_and_scene_diverse() {
    let app = app().await;
    ingest_pair(&app, "SCENE_A", "生成场景 A 的页面，展示列表与筛选。").await;
    ingest_pair(&app, "SCENE_B", "生成场景 B 的页面，展示表单与校验。").await;
    ingest_pair(&app, "SCENE_C", "生成场景 C 的页面，展示图表与汇总。").await;

    let path = "/api/v1/preference/sample?limit=2&seed=review-001";
    let (status, first) = send(&app, with_bearer(get(path), "dev:admin")).await;
    let (status_second, second) = send(&app, with_bearer(get(path), "dev:admin")).await;
    assert_eq!(status, StatusCode::OK, "sample: {first}");
    assert_eq!(status_second, StatusCode::OK);

    let scenes: Vec<&str> = first["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["scene_code"].as_str().unwrap())
        .collect();
    assert_eq!(scenes.len(), 2);
    assert_ne!(scenes[0], scenes[1], "sample should be scene-diverse");
    assert_eq!(
        first["data"]["items"], second["data"]["items"],
        "sample must be deterministic for the same seed"
    );
}

#[tokio::test]
async fn export_hashes_user_and_marks_records_exported() {
    let app = app().await;
    ingest_pair(&app, "ORDER_DETAIL_PAGE", PROMPT).await;

    let (status, exported) = send(
        &app,
        with_bearer(
            post_json("/api/v1/preference/export", json!({})),
            "dev:admin",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "export: {exported}");
    let manifest = &exported["data"]["manifest"];
    assert_eq!(manifest["format"], "dpo-jsonl-v1");
    assert_eq!(manifest["record_count"], 1);
    assert_eq!(manifest["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(manifest["unique_user_count"], 1);

    let record = &exported["data"]["records"][0];
    let user_hash = record["metadata"]["user_hash"].as_str().unwrap();
    assert!(user_hash.starts_with("u_"));
    assert_ne!(user_hash, "u100");
    assert_eq!(record["metadata"]["scene_code"], "ORDER_DETAIL_PAGE");
    assert_eq!(record["metadata"]["prompt_template_version"], "prompt-v1");

    let (status, second) = send(
        &app,
        with_bearer(
            post_json("/api/v1/preference/export", json!({})),
            "dev:admin",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["data"]["manifest"]["record_count"], 0);
}

#[tokio::test]
async fn export_respects_per_user_quota() {
    let app = app().await;
    ingest_pair(&app, "SCENE_A", "生成场景 A 的页面，展示列表与筛选。").await;
    ingest_pair(&app, "SCENE_B", "生成场景 B 的页面，展示表单与校验。").await;
    ingest_pair(&app, "SCENE_C", "生成场景 C 的页面，展示图表与汇总。").await;

    let (status, exported) = send(
        &app,
        with_bearer(
            post_json("/api/v1/preference/export", json!({"max_per_user": 2})),
            "dev:admin",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "export: {exported}");
    assert_eq!(exported["data"]["manifest"]["record_count"], 2);
    assert_eq!(exported["data"]["manifest"]["skipped"]["user_quota"], 1);
}

#[tokio::test]
async fn admin_token_is_enforced_when_configured() {
    let app = app_with(config_with_admin_token("test-preference-admin-token")).await;

    let (status, response) = send(
        &app,
        with_bearer(get("/api/v1/preference/stats"), "dev:u100"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(response["code"], 40300);

    let (status, response) = send(
        &app,
        with_admin(
            with_bearer(get("/api/v1/preference/stats"), "dev:u100"),
            "test-preference-admin-token",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stats: {response}");
}

#[tokio::test]
async fn ingest_requires_authentication() {
    let app = app().await;
    let (status, response) = send(
        &app,
        post_json(
            "/api/v1/preference/pairs",
            pair_body("ORDER_DETAIL_PAGE", PROMPT),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(response["code"], 40100);
}

#[tokio::test]
async fn oversize_prompt_and_non_object_metadata_are_rejected() {
    let app = app().await;
    let mut body = pair_body("ORDER_DETAIL_PAGE", &"长".repeat(9_000));
    body["metadata"] = json!(["not", "an", "object"]);

    let (status, response) = ingest(&app, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["code"], 40000);
}
