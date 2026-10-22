//! 原子能力的数据访问实现（PRD §3.3）：
//! `create_order` / `query_user_balance` / `update_item_status` / `delete_record`。
//!
//! 每个写操作与操作记录在同一事务内提交，保证审计与业务数据一致。

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::models::{
    cents_to_decimal, CreateOrderInput, DeletedRecord, OperationRecord, Order, OrderItem,
    UserBalance,
};

use super::{map_db_error, new_id, ProductRow, Repository, RepositoryError, UserRow};

/// `update_item_status` 允许的目标状态。
const ITEM_STATUSES: [&str; 4] = ["NORMAL", "SHIPPED", "REFUNDED", "CANCELLED"];

/// `delete_record` 允许删除的实体类型。
const DELETABLE_ENTITIES: [&str; 4] = ["order", "order_item", "product", "operation_record"];

impl Repository {
    /// `create_order`：按商品快照创建订单与订单项，并写入审计记录。
    pub async fn create_order(&self, input: &CreateOrderInput) -> Result<Order, RepositoryError> {
        if input.items.is_empty() {
            return Err(RepositoryError::Invalid(
                "order must contain at least one item".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await.map_err(map_db_error)?;

        let user: Option<UserRow> = sqlx::query_as(
            "SELECT id, display_name, balance_cents, currency, status, created_at, updated_at
             FROM users WHERE id = ?",
        )
        .bind(&input.user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_db_error)?;

        let user = match user {
            None => {
                return Err(RepositoryError::NotFound {
                    entity: "user",
                    id: input.user_id.clone(),
                })
            }
            Some(user) if user.status != "ACTIVE" => {
                return Err(RepositoryError::Invalid(format!(
                    "user {} is not active",
                    user.id
                )))
            }
            Some(user) => user,
        };
        let currency = user.currency;

        let order_id = match input.order_id.as_deref().map(str::trim) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => generate_order_id(),
        };
        let now = Utc::now();

        let mut total_cents: i64 = 0;
        let mut lines: Vec<PendingOrderLine> = Vec::with_capacity(input.items.len());

        for item in &input.items {
            if item.qty <= 0 {
                return Err(RepositoryError::Invalid(
                    "item qty must be greater than 0".to_string(),
                ));
            }

            let product: Option<ProductRow> = sqlx::query_as(
                "SELECT id, name, price_cents, currency, status, created_at, updated_at
                 FROM products WHERE id = ?",
            )
            .bind(&item.product_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db_error)?;

            let product = product.ok_or_else(|| RepositoryError::NotFound {
                entity: "product",
                id: item.product_id.clone(),
            })?;

            if product.status != "ON_SALE" {
                return Err(RepositoryError::Invalid(format!(
                    "product {} is not on sale",
                    product.id
                )));
            }

            if product.currency != currency {
                return Err(RepositoryError::Invalid(format!(
                    "product {} currency {} does not match user currency {}",
                    product.id, product.currency, currency
                )));
            }

            let amount_cents = product
                .price_cents
                .checked_mul(i64::from(item.qty))
                .ok_or_else(|| RepositoryError::Invalid("order amount overflow".to_string()))?;
            total_cents = total_cents
                .checked_add(amount_cents)
                .ok_or_else(|| RepositoryError::Invalid("order amount overflow".to_string()))?;

            lines.push(PendingOrderLine {
                id: new_id("ITM"),
                product_id: product.id,
                name: product.name,
                unit_price_cents: product.price_cents,
                qty: item.qty,
                amount_cents,
            });
        }

        sqlx::query(
            "INSERT INTO orders
                 (id, user_id, status, total_cents, currency, shipping_address, created_at, updated_at)
             VALUES (?, ?, 'PENDING', ?, ?, ?, ?, ?)",
        )
        .bind(&order_id)
        .bind(&input.user_id)
        .bind(total_cents)
        .bind(&currency)
        .bind(&input.shipping_address)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_db_error)?;

        for line in &lines {
            sqlx::query(
                "INSERT INTO order_items
                     (id, order_id, product_id, name, unit_price_cents, qty, amount_cents,
                      status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, 'NORMAL', ?, ?)",
            )
            .bind(&line.id)
            .bind(&order_id)
            .bind(&line.product_id)
            .bind(&line.name)
            .bind(line.unit_price_cents)
            .bind(line.qty)
            .bind(line.amount_cents)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(map_db_error)?;
        }

        let request_payload = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
        let result_payload = json!({
            "order_id": order_id,
            "status": "PENDING",
            "total_amount": cents_to_decimal(total_cents).to_string(),
        })
        .to_string();

        sqlx::query(
            "INSERT INTO operation_records
                 (id, actor_user_id, capability, entity_type, entity_id,
                  request_payload, result_payload, status, created_at)
             VALUES (?, ?, 'create_order', 'order', ?, ?, ?, 'SUCCESS', ?)",
        )
        .bind(new_id("OPR"))
        .bind(&input.user_id)
        .bind(&order_id)
        .bind(request_payload)
        .bind(result_payload)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_db_error)?;

        tx.commit().await.map_err(map_db_error)?;

        self.find_order(&order_id).await
    }

    /// `query_user_balance`：查询余额并写入审计记录。
    pub async fn query_user_balance(&self, user_id: &str) -> Result<UserBalance, RepositoryError> {
        let balance = self.find_user_balance(user_id).await?;

        let request_payload = json!({ "user_id": user_id }).to_string();
        let result_payload = json!({
            "user_id": balance.user_id,
            "balance": balance.balance.to_string(),
            "currency": balance.currency,
        })
        .to_string();

        self.record_operation(
            "query_user_balance",
            Some(user_id),
            "user",
            user_id,
            &request_payload,
            &result_payload,
        )
        .await?;

        Ok(balance)
    }

    /// `update_item_status`：更新订单项状态并写入审计记录。
    pub async fn update_item_status(
        &self,
        actor_user_id: Option<&str>,
        item_id: &str,
        status: &str,
    ) -> Result<OrderItem, RepositoryError> {
        if !ITEM_STATUSES.contains(&status) {
            return Err(RepositoryError::Invalid(format!(
                "unsupported item status `{status}`"
            )));
        }

        let result = sqlx::query("UPDATE order_items SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(Utc::now())
            .bind(item_id)
            .execute(&self.pool)
            .await
            .map_err(map_db_error)?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound {
                entity: "order_item",
                id: item_id.to_string(),
            });
        }

        let item = self.find_order_item(item_id).await?;
        let request_payload = json!({ "item_id": item_id, "status": status }).to_string();
        let result_payload = serde_json::to_string(&item).unwrap_or_else(|_| "{}".to_string());

        self.record_operation(
            "update_item_status",
            actor_user_id,
            "order_item",
            item_id,
            &request_payload,
            &result_payload,
        )
        .await?;

        Ok(item)
    }

    /// `delete_record`：删除指定实体并写入审计记录。
    pub async fn delete_record(
        &self,
        actor_user_id: Option<&str>,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<DeletedRecord, RepositoryError> {
        if !DELETABLE_ENTITIES.contains(&entity_type) {
            return Err(RepositoryError::Invalid(format!(
                "unsupported entity type `{entity_type}`"
            )));
        }

        let mut tx = self.pool.begin().await.map_err(map_db_error)?;

        let sql = match entity_type {
            "order" => "DELETE FROM orders WHERE id = ?",
            "order_item" => "DELETE FROM order_items WHERE id = ?",
            "product" => "DELETE FROM products WHERE id = ?",
            "operation_record" => "DELETE FROM operation_records WHERE id = ?",
            _ => unreachable!("entity type validated above"),
        };

        let result = sqlx::query(sql)
            .bind(entity_id)
            .execute(&mut *tx)
            .await
            .map_err(map_db_error)?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound {
                entity: "record",
                id: entity_id.to_string(),
            });
        }

        let request_payload =
            json!({ "entity_type": entity_type, "entity_id": entity_id }).to_string();
        let result_payload = json!({ "deleted": true }).to_string();

        sqlx::query(
            "INSERT INTO operation_records
                 (id, actor_user_id, capability, entity_type, entity_id,
                  request_payload, result_payload, status, created_at)
             VALUES (?, ?, 'delete_record', ?, ?, ?, ?, 'SUCCESS', ?)",
        )
        .bind(new_id("OPR"))
        .bind(actor_user_id)
        .bind(entity_type)
        .bind(entity_id)
        .bind(request_payload)
        .bind(result_payload)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await
        .map_err(map_db_error)?;

        tx.commit().await.map_err(map_db_error)?;

        Ok(DeletedRecord {
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            deleted: true,
        })
    }

    /// 查询某实体的操作记录（审计排查 / 测试使用）。
    pub async fn list_operation_records(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<Vec<OperationRecord>, RepositoryError> {
        let rows: Vec<OperationRecordRow> = sqlx::query_as(
            "SELECT id, actor_user_id, capability, entity_type, entity_id,
                    request_payload, result_payload, status, created_at
             FROM operation_records
             WHERE entity_type = ? AND entity_id = ?
             ORDER BY created_at ASC, id ASC",
        )
        .bind(entity_type)
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(rows
            .into_iter()
            .map(OperationRecordRow::into_record)
            .collect())
    }
}

/// 创建订单时暂存的商品行。
struct PendingOrderLine {
    id: String,
    product_id: String,
    name: String,
    unit_price_cents: i64,
    qty: i32,
    amount_cents: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct OperationRecordRow {
    id: String,
    actor_user_id: Option<String>,
    capability: String,
    entity_type: String,
    entity_id: String,
    request_payload: String,
    result_payload: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
}

impl OperationRecordRow {
    fn into_record(self) -> OperationRecord {
        OperationRecord {
            id: self.id,
            actor_user_id: self.actor_user_id,
            capability: self.capability,
            entity_type: self.entity_type,
            entity_id: self.entity_id,
            request_payload: self.request_payload,
            result_payload: self.result_payload,
            status: self.status,
            created_at: self.created_at,
        }
    }
}

/// 生成订单号：`ORD_<YYYYMMDD>_<8 位随机 hex>`。
fn generate_order_id() -> String {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("ORD_{}_{}", Utc::now().format("%Y%m%d"), &suffix[..8])
}
