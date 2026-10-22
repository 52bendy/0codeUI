//! 订单与订单项查询。

use crate::models::{Order, OrderBusinessData, OrderItem};

use super::{map_db_error, OrderItemRow, OrderRow, OrderWithItems, Repository, RepositoryError};

impl Repository {
    /// 按订单号查询订单（不存在时返回 [`RepositoryError::NotFound`]）。
    pub async fn find_order(&self, order_id: &str) -> Result<Order, RepositoryError> {
        let row: Option<OrderRow> = sqlx::query_as(
            "SELECT id, user_id, status, total_cents, currency, shipping_address,
                    created_at, updated_at
             FROM orders WHERE id = ?",
        )
        .bind(order_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        row.map(OrderRow::into_order)
            .ok_or_else(|| RepositoryError::NotFound {
                entity: "order",
                id: order_id.to_string(),
            })
    }

    /// 查询某用户的全部订单（按创建时间倒序）。
    pub async fn list_orders_for_user(&self, user_id: &str) -> Result<Vec<Order>, RepositoryError> {
        let rows: Vec<OrderRow> = sqlx::query_as(
            "SELECT id, user_id, status, total_cents, currency, shipping_address,
                    created_at, updated_at
             FROM orders WHERE user_id = ? ORDER BY created_at DESC, id DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(rows.into_iter().map(OrderRow::into_order).collect())
    }

    /// 按订单项 ID 查询订单项。
    pub async fn find_order_item(&self, item_id: &str) -> Result<OrderItem, RepositoryError> {
        let row: Option<OrderItemRow> = sqlx::query_as(
            "SELECT id, order_id, product_id, name, unit_price_cents, qty, amount_cents,
                    status, created_at, updated_at
             FROM order_items WHERE id = ?",
        )
        .bind(item_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        row.map(OrderItemRow::into_item)
            .ok_or_else(|| RepositoryError::NotFound {
                entity: "order_item",
                id: item_id.to_string(),
            })
    }

    /// 查询订单及其订单项（内部使用）。
    pub(crate) async fn find_order_with_items(
        &self,
        order_id: &str,
    ) -> Result<OrderWithItems, RepositoryError> {
        let order = self.find_order(order_id).await?;

        let rows: Vec<OrderItemRow> = sqlx::query_as(
            "SELECT id, order_id, product_id, name, unit_price_cents, qty, amount_cents,
                    status, created_at, updated_at
             FROM order_items WHERE order_id = ? ORDER BY id",
        )
        .bind(order_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(OrderWithItems {
            order,
            items: rows.into_iter().map(OrderItemRow::into_item).collect(),
        })
    }

    /// 示例订单查询：返回与 PRD §5.1 `business_data` 完全同构的结构。
    pub async fn order_business_data(
        &self,
        order_id: &str,
    ) -> Result<OrderBusinessData, RepositoryError> {
        let result = self.find_order_with_items(order_id).await?;
        Ok(result.into_business_data())
    }
}
