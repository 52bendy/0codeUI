//! 原子能力注册中心集成测试（REQ-202610-1178）。
//!
//! 覆盖：能力清单、四个首批能力的执行、参数校验、权限校验、
//! 审计记录（操作人 / 参数摘要 / 结果）与 HTTP 契约。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::capability::{self, CapabilityError, ExecutionContext};
use intimeui_server::config::AppConfig;
use intimeui_server::db::Db;
use intimeui_server::repository::Repository;
use intimeui_server::state::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn seeded_db() -> Db {
    let db = Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to migrate");
    db.seed().await.expect("failed to seed");
    db
}

async fn app_with_db() -> (axum::Router, Db) {
    let db = seeded_db().await;
    let config = AppConfig {
        rate_limit_enabled: false,
        ..AppConfig::default()
    };
    (build_app(AppState::new(config, db.clone())), db)
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

#[test]
fn registry_lists_four_first_batch_capabilities() {
    let descriptors = capability::descriptors();
    let names: Vec<&str> = descriptors.iter().map(|item| item.name).collect();
    assert_eq!(
        names,
        vec![
            "create_order",
            "query_user_balance",
            "update_item_status",
            "delete_record"
        ]
    );

    for descriptor in &descriptors {
        assert!(!descriptor.description.is_empty());
        assert!(!descriptor.required_permissions.is_empty());
        assert_eq!(descriptor.parameters["type"], "object");
        assert_eq!(descriptor.parameters["additionalProperties"], false);
        assert_eq!(descriptor.function["name"], descriptor.name);
        assert_eq!(descriptor.function["parameters"], descriptor.parameters);
        assert!(descriptor.returns["type"].is_string());
    }

    let create_order = descriptors
        .iter()
        .find(|item| item.name == "create_order")
        .expect("create_order must be registered");
    assert_eq!(create_order.risk_level, capability::RiskLevel::Write);
    assert_eq!(create_order.parameters["required"], json!(["items"]));

    let delete_record = descriptors
        .iter()
        .find(|item| item.name == "delete_record")
        .expect("delete_record must be registered");
    assert_eq!(delete_record.risk_level, capability::RiskLevel::Delete);
}

#[tokio::test]
async fn execute_create_order_validates_and_audits() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);
    let context = ExecutionContext::authenticated("usr_9527", false);

    let outcome = capability::execute(
        &repository,
        &context,
        "create_order",
        &json!({
            "order_id": "ORD_CAP_0001",
            "shipping_address": "北京市海淀区中关村南大街1号",
            "items": [{"product_id": "PROD_20261014_001", "qty": 2}]
        }),
    )
    .await
    .expect("create_order should succeed");

    assert_eq!(outcome.capability, "create_order");
    assert_eq!(outcome.risk_level, capability::RiskLevel::Write);
    assert_eq!(outcome.result["id"], "ORD_CAP_0001");
    assert_eq!(outcome.result["status"], "PENDING");
    assert_eq!(outcome.result["total_amount"].to_string(), "598.00");

    let records = repository
        .list_operation_records("order", "ORD_CAP_0001")
        .await
        .unwrap();
    let record = records
        .iter()
        .find(|item| item.capability == "create_order")
        .expect("create_order audit record must exist");
    assert_eq!(record.actor_user_id.as_deref(), Some("usr_9527"));
    assert_eq!(record.status, "SUCCESS");
    assert!(record.request_payload.contains("PROD_20261014_001"));
    assert!(record.result_payload.as_deref().unwrap().contains("598.00"));
}

#[tokio::test]
async fn execute_query_user_balance_is_audited() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);
    let context = ExecutionContext::authenticated("usr_9527", false);

    let outcome = capability::execute(&repository, &context, "query_user_balance", &json!({}))
        .await
        .expect("query_user_balance should succeed");
    assert_eq!(outcome.result["user_id"], "usr_9527");
    assert_eq!(outcome.result["balance"].to_string(), "1000.00");
    assert_eq!(outcome.result["currency"], "CNY");

    let records = repository
        .list_operation_records("user", "usr_9527")
        .await
        .unwrap();
    let record = records
        .iter()
        .find(|item| item.capability == "query_user_balance")
        .expect("query_user_balance audit record must exist");
    assert_eq!(record.actor_user_id.as_deref(), Some("usr_9527"));
    assert_eq!(record.status, "SUCCESS");
}

#[tokio::test]
async fn execute_update_item_status_is_audited() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);
    let context = ExecutionContext::authenticated("usr_9527", false);

    let outcome = capability::execute(
        &repository,
        &context,
        "update_item_status",
        &json!({"item_id": "ITM_20261014_001", "status": "SHIPPED"}),
    )
    .await
    .expect("update_item_status should succeed");
    assert_eq!(outcome.result["id"], "ITM_20261014_001");
    assert_eq!(outcome.result["status"], "SHIPPED");

    let records = repository
        .list_operation_records("order_item", "ITM_20261014_001")
        .await
        .unwrap();
    assert!(records.iter().any(|item| {
        item.capability == "update_item_status"
            && item.actor_user_id.as_deref() == Some("usr_9527")
            && item.status == "SUCCESS"
    }));
}

#[tokio::test]
async fn execute_delete_record_is_audited() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);
    let context = ExecutionContext::authenticated("usr_9527", false);

    let outcome = capability::execute(
        &repository,
        &context,
        "delete_record",
        &json!({"entity_type": "order", "entity_id": "ORD_20261015_002"}),
    )
    .await
    .expect("delete_record should succeed");
    assert_eq!(outcome.result["entity_type"], "order");
    assert_eq!(outcome.result["entity_id"], "ORD_20261015_002");
    assert_eq!(outcome.result["deleted"], true);

    let records = repository
        .list_operation_records("order", "ORD_20261015_002")
        .await
        .unwrap();
    assert!(records.iter().any(|item| {
        item.capability == "delete_record"
            && item.actor_user_id.as_deref() == Some("usr_9527")
            && item.status == "SUCCESS"
    }));
}

#[tokio::test]
async fn invalid_parameters_are_rejected_and_audited_as_failed() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);
    let context = ExecutionContext::authenticated("usr_9527", false);

    let error = capability::execute(
        &repository,
        &context,
        "create_order",
        &json!({"order_id": "ORD_CAP_BAD", "items": []}),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, CapabilityError::InvalidParameters(_)));

    let records = repository
        .list_operation_records("order", "ORD_CAP_BAD")
        .await
        .unwrap();
    let record = records
        .iter()
        .find(|item| item.capability == "create_order")
        .expect("failed validation must be audited");
    assert_eq!(record.status, "FAILED");
    assert_eq!(record.actor_user_id.as_deref(), Some("usr_9527"));
    assert!(record.request_payload.contains("ORD_CAP_BAD"));
    assert!(record.result_payload.as_deref().unwrap().contains("error"));
}

#[tokio::test]
async fn unknown_capability_and_missing_permission_are_rejected() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);
    let context = ExecutionContext::authenticated("usr_9527", false);

    let unknown = capability::execute(&repository, &context, "not_a_capability", &json!({}))
        .await
        .unwrap_err();
    assert!(matches!(unknown, CapabilityError::UnknownCapability(_)));

    let restricted = ExecutionContext::with_permissions("usr_9527", Vec::<String>::new());
    let forbidden = capability::execute(&repository, &restricted, "query_user_balance", &json!({}))
        .await
        .unwrap_err();
    assert!(matches!(forbidden, CapabilityError::Forbidden(_)));
}

#[tokio::test]
async fn user_id_must_match_authenticated_user() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);
    let context = ExecutionContext::authenticated("usr_9527", false);

    let error = capability::execute(
        &repository,
        &context,
        "query_user_balance",
        &json!({"user_id": "usr_1001"}),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, CapabilityError::Forbidden(_)));
}

#[tokio::test]
async fn capability_list_endpoint_is_agent_consumable() {
    let (app, _db) = app_with_db().await;
    let (status, body) = send(
        &app,
        with_bearer(get("/api/v1/capabilities"), "dev:usr_9527"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["count"], 4);
    let capabilities = body["data"]["capabilities"].as_array().unwrap();
    assert_eq!(capabilities.len(), 4);
    for item in capabilities {
        assert!(item["function"]["name"].is_string());
        assert_eq!(item["function"]["parameters"]["type"], "object");
        assert!(item["risk_level"].is_string());
        assert!(item["required_permissions"].is_array());
    }
}

#[tokio::test]
async fn execute_endpoint_requires_auth_and_returns_result() {
    let (app, db) = app_with_db().await;
    let path = "/api/v1/capabilities/query_user_balance/execute";

    let (status, body) = send(&app, post_json(path, json!({"parameters": {}}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], 40100);

    let request = with_bearer(post_json(path, json!({"parameters": {}})), "dev:usr_9527");
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["capability"], "query_user_balance");
    assert_eq!(body["data"]["result"]["balance"].to_string(), "1000.00");

    let repository = Repository::from_db(&db);
    let records = repository
        .list_operation_records("user", "usr_9527")
        .await
        .unwrap();
    assert!(records.iter().any(|item| {
        item.capability == "query_user_balance"
            && item.actor_user_id.as_deref() == Some("usr_9527")
            && item.status == "SUCCESS"
    }));
}

#[tokio::test]
async fn execute_endpoint_rejects_invalid_parameters() {
    let (app, _db) = app_with_db().await;
    let path = "/api/v1/capabilities/update_item_status/execute";
    let request = with_bearer(
        post_json(
            path,
            json!({"parameters": {"item_id": "ITM_20261014_001", "status": "BOGUS"}}),
        ),
        "dev:usr_9527",
    );

    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
}

#[tokio::test]
async fn execute_endpoint_reports_unknown_capability_and_scope_mismatch() {
    let (app, _db) = app_with_db().await;

    let request = with_bearer(
        post_json(
            "/api/v1/capabilities/not_a_capability/execute",
            json!({"parameters": {}}),
        ),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], 40400);

    let request = with_bearer(
        post_json(
            "/api/v1/capabilities/query_user_balance/execute",
            json!({"parameters": {"user_id": "usr_1001"}}),
        ),
        "dev:usr_9527",
    );
    let (status, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], 40300);
}
