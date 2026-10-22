//! 用户查询。

use crate::models::{User, UserBalance};

use super::{map_db_error, Repository, RepositoryError, UserRow};

impl Repository {
    /// 按用户 ID 查询用户。
    pub async fn find_user(&self, user_id: &str) -> Result<User, RepositoryError> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT id, display_name, balance_cents, currency, status, created_at, updated_at
             FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        row.map(UserRow::into_user)
            .ok_or_else(|| RepositoryError::NotFound {
                entity: "user",
                id: user_id.to_string(),
            })
    }

    /// 查询用户余额（`query_user_balance` 原子能力的数据源）。
    pub async fn find_user_balance(&self, user_id: &str) -> Result<UserBalance, RepositoryError> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT id, display_name, balance_cents, currency, status, created_at, updated_at
             FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        row.map(UserRow::into_balance)
            .ok_or_else(|| RepositoryError::NotFound {
                entity: "user",
                id: user_id.to_string(),
            })
    }
}
