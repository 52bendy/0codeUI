//! 商品查询。

use crate::models::Product;

use super::{map_db_error, ProductRow, Repository, RepositoryError};

impl Repository {
    /// 按商品 ID 查询商品。
    pub async fn find_product(&self, product_id: &str) -> Result<Product, RepositoryError> {
        let row: Option<ProductRow> = sqlx::query_as(
            "SELECT id, name, price_cents, currency, status, created_at, updated_at
             FROM products WHERE id = ?",
        )
        .bind(product_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        row.map(ProductRow::into_product)
            .ok_or_else(|| RepositoryError::NotFound {
                entity: "product",
                id: product_id.to_string(),
            })
    }

    /// 列出全部商品（按 ID 排序）。
    pub async fn list_products(&self) -> Result<Vec<Product>, RepositoryError> {
        let rows: Vec<ProductRow> = sqlx::query_as(
            "SELECT id, name, price_cents, currency, status, created_at, updated_at
             FROM products ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(rows.into_iter().map(ProductRow::into_product).collect())
    }
}
