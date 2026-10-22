//! 数据模型 / 迁移 / 种子数据 / 原子能力集成测试（REQ-202610-1170）。

use intimeui_server::db::Db;
use intimeui_server::models::{
    decimal_to_cents, CreateOrderInput, CreateOrderItemInput, MoneyError, SeedReport,
};
use intimeui_server::repository::{Repository, RepositoryError};
use rust_decimal::Decimal;

async fn seeded_db() -> Db {
    let db = Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to migrate");
    db.seed().await.expect("failed to seed");
    db
}

#[tokio::test]
async fn migrations_are_repeatable_and_idempotent() {
    let db = Db::connect_in_memory().await.unwrap();

    db.migrate().await.expect("first migration run");
    db.migrate().await.expect("second migration run");

    // 迁移后核心表均可查询，说明重复执行没有破坏 schema。
    let repository = Repository::from_db(&db);
    assert!(repository.find_user("usr_9527").await.is_err());
}

#[tokio::test]
async fn seed_data_is_one_click_and_idempotent() {
    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();

    let first = db.seed().await.expect("first seed run");
    assert_eq!(
        first,
        SeedReport {
            users: 2,
            products: 2,
            orders: 2,
            order_items: 2,
            operation_records: 1,
        }
    );
    assert_eq!(first.total(), 9);

    let second = db.seed().await.expect("second seed run");
    assert_eq!(second, SeedReport::default(), "seed must be idempotent");
}

#[tokio::test]
async fn order_business_data_matches_prd_5_1() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);

    let data = repository
        .order_business_data("ORD_20261014_001")
        .await
        .expect("sample order must exist");

    assert_eq!(data.order_id, "ORD_20261014_001");
    assert_eq!(data.status, "PAID");
    assert_eq!(data.items.len(), 1);
    assert_eq!(data.items[0].name, "智能降噪耳机");
    assert_eq!(data.items[0].price, Decimal::new(29900, 2));
    assert_eq!(data.items[0].qty, 1);

    // JSON 结构与 PRD §5.1 business_data 一致，金额为十进制数字。
    let json = serde_json::to_value(&data).unwrap();
    assert_eq!(json["order_id"], "ORD_20261014_001");
    assert_eq!(json["status"], "PAID");
    assert_eq!(json["items"][0]["name"], "智能降噪耳机");
    assert_eq!(json["items"][0]["price"].to_string(), "299.00");
    assert_eq!(json["items"][0]["qty"], 1);
    assert_eq!(
        json.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["items", "order_id", "status"]
    );
    assert_eq!(
        json["items"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["name", "price", "qty"]
    );
}

#[tokio::test]
async fn user_balance_uses_decimal_and_is_audited() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);

    let balance = repository
        .query_user_balance("usr_9527")
        .await
        .expect("seed user must exist");
    assert_eq!(balance.user_id, "usr_9527");
    assert_eq!(balance.balance, Decimal::new(100000, 2));
    assert_eq!(balance.currency, "CNY");

    let records = repository
        .list_operation_records("user", "usr_9527")
        .await
        .unwrap();
    assert!(records
        .iter()
        .any(|record| record.capability == "query_user_balance"));
}

#[tokio::test]
async fn create_order_snapshots_items_and_writes_audit() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);

    let input = CreateOrderInput {
        user_id: "usr_9527".to_string(),
        order_id: Some("ORD_TEST_0001".to_string()),
        shipping_address: Some("北京市海淀区中关村南大街1号".to_string()),
        items: vec![CreateOrderItemInput {
            product_id: "PROD_20261014_001".to_string(),
            qty: 2,
        }],
    };

    let order = repository.create_order(&input).await.expect("create order");
    assert_eq!(order.id, "ORD_TEST_0001");
    assert_eq!(order.user_id, "usr_9527");
    assert_eq!(order.status, "PENDING");
    assert_eq!(order.total_amount, Decimal::new(59800, 2));

    let data = repository
        .order_business_data("ORD_TEST_0001")
        .await
        .unwrap();
    assert_eq!(data.items.len(), 1);
    assert_eq!(data.items[0].name, "智能降噪耳机");
    assert_eq!(data.items[0].price, Decimal::new(29900, 2));
    assert_eq!(data.items[0].qty, 2);

    let records = repository
        .list_operation_records("order", "ORD_TEST_0001")
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].capability, "create_order");
    assert_eq!(records[0].status, "SUCCESS");

    // 重复订单号 → 409 冲突，且事务回滚（不会留下半个订单）。
    let duplicate = repository.create_order(&input).await.unwrap_err();
    assert!(matches!(duplicate, RepositoryError::Conflict(_)));
}

#[tokio::test]
async fn create_order_rejects_invalid_input() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);

    let empty = CreateOrderInput {
        user_id: "usr_9527".to_string(),
        order_id: None,
        shipping_address: None,
        items: vec![],
    };
    assert!(matches!(
        repository.create_order(&empty).await.unwrap_err(),
        RepositoryError::Invalid(_)
    ));

    let unknown_product = CreateOrderInput {
        user_id: "usr_9527".to_string(),
        order_id: None,
        shipping_address: None,
        items: vec![CreateOrderItemInput {
            product_id: "PROD_MISSING".to_string(),
            qty: 1,
        }],
    };
    assert!(matches!(
        repository.create_order(&unknown_product).await.unwrap_err(),
        RepositoryError::NotFound { .. }
    ));

    let zero_qty = CreateOrderInput {
        user_id: "usr_9527".to_string(),
        order_id: None,
        shipping_address: None,
        items: vec![CreateOrderItemInput {
            product_id: "PROD_20261014_001".to_string(),
            qty: 0,
        }],
    };
    assert!(matches!(
        repository.create_order(&zero_qty).await.unwrap_err(),
        RepositoryError::Invalid(_)
    ));
}

#[tokio::test]
async fn update_item_status_updates_and_audits() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);

    let item = repository
        .update_item_status(Some("usr_9527"), "ITM_20261014_001", "SHIPPED")
        .await
        .expect("update item status");
    assert_eq!(item.status, "SHIPPED");

    let invalid = repository
        .update_item_status(None, "ITM_20261014_001", "BOGUS")
        .await
        .unwrap_err();
    assert!(matches!(invalid, RepositoryError::Invalid(_)));

    let records = repository
        .list_operation_records("order_item", "ITM_20261014_001")
        .await
        .unwrap();
    assert!(records
        .iter()
        .any(|record| record.capability == "update_item_status"));
}

#[tokio::test]
async fn delete_record_cascades_and_audits() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);

    let deleted = repository
        .delete_record(Some("usr_9527"), "order", "ORD_20261015_002")
        .await
        .expect("delete order");
    assert!(deleted.deleted);

    // 订单项随订单级联删除。
    assert!(matches!(
        repository.find_order("ORD_20261015_002").await.unwrap_err(),
        RepositoryError::NotFound { .. }
    ));
    assert!(matches!(
        repository
            .find_order_item("ITM_20261015_001")
            .await
            .unwrap_err(),
        RepositoryError::NotFound { .. }
    ));

    let records = repository
        .list_operation_records("order", "ORD_20261015_002")
        .await
        .unwrap();
    assert!(records
        .iter()
        .any(|record| record.capability == "delete_record"));

    let invalid = repository
        .delete_record(None, "unknown", "x")
        .await
        .unwrap_err();
    assert!(matches!(invalid, RepositoryError::Invalid(_)));
}

#[tokio::test]
async fn product_delete_keeps_order_item_snapshot() {
    let db = seeded_db().await;
    let repository = Repository::from_db(&db);

    repository
        .delete_record(None, "product", "PROD_20261014_001")
        .await
        .expect("delete product");

    let item = repository
        .find_order_item("ITM_20261014_001")
        .await
        .unwrap();
    assert_eq!(item.product_id, None);
    assert_eq!(item.name, "智能降噪耳机");
    assert_eq!(item.unit_price, Decimal::new(29900, 2));
}

#[test]
fn money_conversion_never_uses_floats() {
    assert_eq!(decimal_to_cents(Decimal::new(29900, 2)).unwrap(), 29900);
    assert_eq!(
        decimal_to_cents(Decimal::new(1005, 3)),
        Err(MoneyError::TooManyDecimalPlaces)
    );
}
