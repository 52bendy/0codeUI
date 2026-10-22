//! 示例种子数据：对齐 PRD §5.1（user `usr_9527`、订单 `ORD_20261014_001`、
//! 智能降噪耳机 299.00 × 1）。
//!
//! 所有插入均使用 `INSERT OR IGNORE`，重复加载不会产生重复数据。

use sqlx::SqlitePool;

use crate::models::SeedReport;

/// 加载种子数据，返回本次实际插入的行数。
pub async fn load(pool: &SqlitePool) -> Result<SeedReport, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut report = SeedReport::default();

    // ---- 用户 ----
    report.users += insert(
        &mut tx,
        "INSERT OR IGNORE INTO users
             (id, display_name, balance_cents, currency, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        &[
            "usr_9527",
            "示例用户",
            "100000",
            "CNY",
            "ACTIVE",
            "2026-10-14T09:00:00Z",
            "2026-10-14T09:00:00Z",
        ],
    )
    .await?;

    report.users += insert(
        &mut tx,
        "INSERT OR IGNORE INTO users
             (id, display_name, balance_cents, currency, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        &[
            "usr_1001",
            "测试用户",
            "50000",
            "CNY",
            "ACTIVE",
            "2026-10-14T09:05:00Z",
            "2026-10-14T09:05:00Z",
        ],
    )
    .await?;

    // ---- 商品 ----
    report.products += insert(
        &mut tx,
        "INSERT OR IGNORE INTO products
             (id, name, price_cents, currency, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        &[
            "PROD_20261014_001",
            "智能降噪耳机",
            "29900",
            "CNY",
            "ON_SALE",
            "2026-10-14T09:10:00Z",
            "2026-10-14T09:10:00Z",
        ],
    )
    .await?;

    report.products += insert(
        &mut tx,
        "INSERT OR IGNORE INTO products
             (id, name, price_cents, currency, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        &[
            "PROD_20261015_002",
            "机械键盘",
            "49900",
            "CNY",
            "ON_SALE",
            "2026-10-14T09:12:00Z",
            "2026-10-14T09:12:00Z",
        ],
    )
    .await?;

    // ---- 订单 ----
    report.orders += insert(
        &mut tx,
        "INSERT OR IGNORE INTO orders
             (id, user_id, status, total_cents, currency, shipping_address, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        &[
            "ORD_20261014_001",
            "usr_9527",
            "PAID",
            "29900",
            "CNY",
            "北京市海淀区中关村南大街1号",
            "2026-10-14T10:15:00Z",
            "2026-10-14T10:15:00Z",
        ],
    )
    .await?;

    report.orders += insert(
        &mut tx,
        "INSERT OR IGNORE INTO orders
             (id, user_id, status, total_cents, currency, shipping_address, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        &[
            "ORD_20261015_002",
            "usr_9527",
            "PENDING",
            "99800",
            "CNY",
            "北京市海淀区中关村南大街1号",
            "2026-10-15T09:30:00Z",
            "2026-10-15T09:30:00Z",
        ],
    )
    .await?;

    // ---- 订单项 ----
    report.order_items += insert(
        &mut tx,
        "INSERT OR IGNORE INTO order_items
             (id, order_id, product_id, name, unit_price_cents, qty, amount_cents, status,
              created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        &[
            "ITM_20261014_001",
            "ORD_20261014_001",
            "PROD_20261014_001",
            "智能降噪耳机",
            "29900",
            "1",
            "29900",
            "NORMAL",
            "2026-10-14T10:15:00Z",
            "2026-10-14T10:15:00Z",
        ],
    )
    .await?;

    report.order_items += insert(
        &mut tx,
        "INSERT OR IGNORE INTO order_items
             (id, order_id, product_id, name, unit_price_cents, qty, amount_cents, status,
              created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        &[
            "ITM_20261015_001",
            "ORD_20261015_002",
            "PROD_20261015_002",
            "机械键盘",
            "49900",
            "2",
            "99800",
            "NORMAL",
            "2026-10-15T09:30:00Z",
            "2026-10-15T09:30:00Z",
        ],
    )
    .await?;

    // ---- 操作记录 ----
    report.operation_records += insert(
        &mut tx,
        "INSERT OR IGNORE INTO operation_records
             (id, actor_user_id, capability, entity_type, entity_id, request_payload,
              result_payload, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        &[
            "OPR_20261014_001",
            "usr_9527",
            "create_order",
            "order",
            "ORD_20261014_001",
            r#"{"order_id":"ORD_20261014_001","items":[{"product_id":"PROD_20261014_001","qty":1}]}"#,
            r#"{"status":"PAID","total_amount":299.00}"#,
            "SUCCESS",
            "2026-10-14T10:15:00Z",
        ],
    )
    .await?;

    tx.commit().await?;
    Ok(report)
}

/// 执行单条 `INSERT OR IGNORE` 并返回实际插入行数。
async fn insert(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    sql: &str,
    values: &[&str],
) -> Result<usize, sqlx::Error> {
    let mut query = sqlx::query(sql);
    for value in values {
        query = query.bind(*value);
    }
    Ok(query.execute(&mut **tx).await?.rows_affected() as usize)
}
