//! 安全风控与二次确认流集成测试（REQ-202610-1179）。
//!
//! 覆盖验收标准：
//! - 写 / 删类意图未确认前绝不执行；
//! - 确认后仅执行一次，重放返回 409；
//! - 伪造令牌、跨用户、跨会话、过期确认全部拒绝；
//! - 拒绝 / 取消不产生副作用；
//! - 读类意图不受影响；
//! - 全生命周期审计事件完整。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::config::AppConfig;
use intimeui_server::db::Db;
use intimeui_server::guardrails::ConfirmationService;
use intimeui_server::repository::Repository;
use intimeui_server::state::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_config() -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        ..AppConfig::default()
    }
}

async fn app_and_db(config: AppConfig) -> (axum::Router, Db) {
    let db = Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to run migrations");
    db.seed().await.expect("failed to load seed data");
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

fn post_json(path: &str, body: Value, token: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    request
}

fn execute_intent(body: Value, token: &str) -> Request<Body> {
    post_json("/api/v1/intent/execute", body, token)
}

fn address_intent(address: &str) -> Value {
    json!({
        "user_id": "usr_9527",
        "input_type": "NATURAL_LANGUAGE",
        "raw_input": format!("把这个订单的收货地址修改为{address}"),
        "current_context": {
            "active_page": "ORDER_DETAIL_PAGE",
            "entity_id": "ORD_20261014_001"
        }
    })
}

/// 创建一笔写类确认单，返回 `(confirmation_id, confirmation_token, session_id)`。
async fn create_address_confirmation(
    app: &axum::Router,
    address: &str,
) -> (String, String, String) {
    let (status, body) = send(app, execute_intent(address_intent(address), "dev:usr_9527")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["data"]["action_type"], "REQUIRE_CONFIRMATION");
    let confirmation = &body["data"]["confirmation"];
    assert_eq!(confirmation["risk_level"], "WRITE");
    assert_eq!(confirmation["expires_in_secs"], 300);
    assert!(confirmation["confirmation_token"]
        .as_str()
        .is_some_and(|token| token.starts_with("cct_")));
    (
        confirmation["confirmation_id"]
            .as_str()
            .expect("confirmation_id")
            .to_string(),
        confirmation["confirmation_token"]
            .as_str()
            .expect("confirmation_token")
            .to_string(),
        body["data"]["session_id"]
            .as_str()
            .expect("session_id")
            .to_string(),
    )
}

fn confirm_body(id: &str, token: &str, session_id: Option<&str>) -> Value {
    match session_id {
        Some(session_id) => json!({
            "confirmation_id": id,
            "confirmation_token": token,
            "session_id": session_id
        }),
        None => json!({
            "confirmation_id": id,
            "confirmation_token": token
        }),
    }
}

async fn confirmation_status(db: &Db, id: &str) -> String {
    let repository = Repository::from_db(db);
    repository
        .find_confirmation(id)
        .await
        .unwrap()
        .expect("confirmation exists")
        .status
        .as_str()
        .to_string()
}

#[tokio::test]
async fn write_intent_requires_confirmation_and_executes_exactly_once() {
    let (app, db) = app_and_db(test_config()).await;
    let new_address = "上海市浦东新区世纪大道100号";
    let (id, token, session_id) = create_address_confirmation(&app, new_address).await;

    // 未确认前：数据不变，也没有写操作审计。
    let repository = Repository::from_db(&db);
    let order = repository.find_order("ORD_20261014_001").await.unwrap();
    assert_eq!(
        order.shipping_address.as_deref(),
        Some("北京市海淀区中关村南大街1号")
    );
    assert_eq!(confirmation_status(&db, &id).await, "PENDING");
    assert!(!repository
        .list_operation_records("order", "ORD_20261014_001")
        .await
        .unwrap()
        .iter()
        .any(|record| record.capability == "update_order_address"));

    // 确认后执行一次。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(&id, &token, Some(&session_id)),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["data"]["status"], "EXECUTED");
    assert_eq!(body["data"]["results"][0]["status"], "SUCCESS");
    assert_eq!(
        body["data"]["results"][0]["data"]["shipping_address"],
        new_address
    );

    let order = repository.find_order("ORD_20261014_001").await.unwrap();
    assert_eq!(order.shipping_address.as_deref(), Some(new_address));
    let records = repository
        .list_operation_records("order", "ORD_20261014_001")
        .await
        .unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.capability == "update_order_address")
            .count(),
        1
    );

    // 重放同一令牌：409，且不会再次执行。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(&id, &token, Some(&session_id)),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], 40900);

    let records = repository
        .list_operation_records("order", "ORD_20261014_001")
        .await
        .unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.capability == "update_order_address")
            .count(),
        1
    );

    let events = repository.list_confirmation_events(&id).await.unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert_eq!(kinds, vec!["CREATED", "CONFIRMED", "EXECUTED"]);
}

#[tokio::test]
async fn reject_cancels_without_side_effects_and_blocks_later_confirm() {
    let (app, db) = app_and_db(test_config()).await;
    let (id, token, _session_id) =
        create_address_confirmation(&app, "广州市天河区体育西路1号").await;

    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/reject",
            json!({
                "confirmation_id": id,
                "confirmation_token": token,
                "reason": "用户取消"
            }),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["data"]["status"], "REJECTED");
    assert_eq!(body["data"]["reason"], "用户取消");

    let repository = Repository::from_db(&db);
    let order = repository.find_order("ORD_20261014_001").await.unwrap();
    assert_eq!(
        order.shipping_address.as_deref(),
        Some("北京市海淀区中关村南大街1号")
    );
    assert!(!repository
        .list_operation_records("order", "ORD_20261014_001")
        .await
        .unwrap()
        .iter()
        .any(|record| record.capability == "update_order_address"));

    // 已拒绝的确认单不能再确认。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(&id, &token, None),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], 40900);

    let events = repository.list_confirmation_events(&id).await.unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert_eq!(kinds, vec!["CREATED", "REJECTED"]);
}

#[tokio::test]
async fn forged_token_is_rejected_without_state_change() {
    let (app, db) = app_and_db(test_config()).await;
    let (id, _token, _session_id) =
        create_address_confirmation(&app, "深圳市南山区科技园1号").await;

    let forged = format!("cct_{}", "a".repeat(64));
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(&id, &forged, None),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);
    assert_eq!(confirmation_status(&db, &id).await, "PENDING");

    let repository = Repository::from_db(&db);
    let events = repository.list_confirmation_events(&id).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "CREATED");
}

#[tokio::test]
async fn confirmation_is_bound_to_user_and_session() {
    let (app, db) = app_and_db(test_config()).await;
    let (id, token, session_id) =
        create_address_confirmation(&app, "成都市武侯区天府大道1号").await;

    // 其他用户持有正确令牌也无法确认。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(&id, &token, None),
            "dev:usr_0001",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], 40300);

    // 会话不匹配同样拒绝。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(&id, &token, Some("sess_other")),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    assert_eq!(confirmation_status(&db, &id).await, "PENDING");
    let _ = session_id;
}

#[tokio::test]
async fn expired_confirmation_is_rejected_and_marked_expired() {
    let (app, db) = app_and_db(test_config()).await;
    let (id, token, _session_id) = create_address_confirmation(&app, "杭州市西湖区文三路1号").await;

    // 直接把过期时间改到过去，避免测试等待。
    sqlx::query("UPDATE intent_confirmations SET expires_at = ? WHERE id = ?")
        .bind(Utc::now() - Duration::seconds(10))
        .bind(&id)
        .execute(db.pool())
        .await
        .unwrap();

    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(&id, &token, None),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], 40900);
    assert_eq!(confirmation_status(&db, &id).await, "EXPIRED");

    let repository = Repository::from_db(&db);
    let events = repository.list_confirmation_events(&id).await.unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert_eq!(kinds, vec!["CREATED", "EXPIRED"]);
}

#[tokio::test]
async fn stale_pending_confirmations_are_swept() {
    let (_app, db) = app_and_db(test_config()).await;
    let (id, _token, _session_id) =
        create_address_confirmation(&_app, "南京市鼓楼区中山路1号").await;

    sqlx::query("UPDATE intent_confirmations SET expires_at = ? WHERE id = ?")
        .bind(Utc::now() - Duration::seconds(10))
        .bind(&id)
        .execute(db.pool())
        .await
        .unwrap();

    let service = ConfirmationService::new(Repository::from_db(&db), 300);
    assert_eq!(service.expire_stale(10).await.unwrap(), 1);
    assert_eq!(confirmation_status(&db, &id).await, "EXPIRED");
}

#[tokio::test]
async fn delete_intent_executes_only_after_confirmation() {
    let (app, db) = app_and_db(test_config()).await;
    let repository = Repository::from_db(&db);

    let (status, body) = send(
        &app,
        execute_intent(
            json!({
                "user_id": "usr_9527",
                "input_type": "NATURAL_LANGUAGE",
                "raw_input": "删除订单 ORD_20261015_002"
            }),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["data"]["action_type"], "REQUIRE_CONFIRMATION");
    assert_eq!(body["data"]["confirmation"]["risk_level"], "DELETE");
    assert!(repository.find_order("ORD_20261015_002").await.is_ok());

    let id = body["data"]["confirmation"]["confirmation_id"]
        .as_str()
        .unwrap();
    let token = body["data"]["confirmation"]["confirmation_token"]
        .as_str()
        .unwrap();

    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            confirm_body(id, token, None),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["data"]["status"], "EXECUTED");
    assert!(repository.find_order("ORD_20261015_002").await.is_err());

    let events = repository.list_confirmation_events(id).await.unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert_eq!(kinds, vec!["CREATED", "CONFIRMED", "EXECUTED"]);
}

#[tokio::test]
async fn read_intent_is_unaffected_and_creates_no_confirmation() {
    let (app, db) = app_and_db(test_config()).await;

    let (status, body) = send(
        &app,
        execute_intent(
            json!({
                "user_id": "usr_9527",
                "input_type": "NATURAL_LANGUAGE",
                "raw_input": "查一下我的账户余额"
            }),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["data"]["action_type"], "EXECUTE");
    assert!(body["data"]["confirmation"].is_null());
    assert_eq!(body["data"]["execution_results"][0]["status"], "SUCCESS");

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM intent_confirmations")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn invalid_confirmation_requests_are_rejected() {
    let (app, _db) = app_and_db(test_config()).await;

    // 缺少确认单编号。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            json!({"confirmation_token": format!("cct_{}", "b".repeat(64))}),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    // 编号格式非法。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            json!({
                "confirmation_id": "CNF_not-hex",
                "confirmation_token": format!("cct_{}", "b".repeat(64))
            }),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 40000);

    // 不存在的确认单。
    let (status, body) = send(
        &app,
        post_json(
            "/api/v1/intent/confirm",
            json!({
                "confirmation_id": format!("CNF_{}", "c".repeat(32)),
                "confirmation_token": format!("cct_{}", "b".repeat(64))
            }),
            "dev:usr_9527",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], 40400);
}
