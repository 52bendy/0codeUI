//! 二次确认单的数据访问（REQ-202610-1179）。
//!
//! 关键约束：
//! - 创建确认单与 `CREATED` 审计事件在同一事务内写入；
//! - `claim_confirmation` 通过 `UPDATE ... WHERE status = 'PENDING'`
//!   原子抢占，保证并发确认只有一个请求能进入执行；
//! - 所有状态迁移都带前置状态条件，重复 / 乱序请求不会覆盖终态。

use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::guardrails::model::{
    risk_level_text, ConfirmationEvent, ConfirmationEventType, ConfirmationRecord,
    ConfirmationStatus, NewConfirmation,
};

use super::{map_db_error, new_id, Repository, RepositoryError};

/// 确认单数据库行。
#[derive(Debug, FromRow)]
struct ConfirmationRow {
    id: String,
    user_id: String,
    session_id: String,
    intent_summary: String,
    risk_level: String,
    proposed_changes: String,
    token_hash: String,
    status: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    decided_at: Option<DateTime<Utc>>,
    executed_at: Option<DateTime<Utc>>,
    result_payload: Option<String>,
    failure_reason: Option<String>,
}

impl ConfirmationRow {
    fn into_record(self) -> ConfirmationRecord {
        let status =
            ConfirmationStatus::from_db(&self.status).unwrap_or(ConfirmationStatus::Failed);
        ConfirmationRecord {
            id: self.id,
            user_id: self.user_id,
            session_id: self.session_id,
            intent_summary: self.intent_summary,
            risk_level: self.risk_level,
            proposed_changes: self.proposed_changes,
            token_hash: self.token_hash,
            status,
            created_at: self.created_at,
            expires_at: self.expires_at,
            decided_at: self.decided_at,
            executed_at: self.executed_at,
            result_payload: self.result_payload,
            failure_reason: self.failure_reason,
        }
    }
}

#[derive(Debug, FromRow)]
struct ConfirmationEventRow {
    id: String,
    confirmation_id: String,
    event_type: String,
    actor_user_id: Option<String>,
    detail: Option<String>,
    created_at: DateTime<Utc>,
}

impl ConfirmationEventRow {
    fn into_event(self) -> ConfirmationEvent {
        ConfirmationEvent {
            id: self.id,
            confirmation_id: self.confirmation_id,
            event_type: self.event_type,
            actor_user_id: self.actor_user_id,
            detail: self.detail,
            created_at: self.created_at,
        }
    }
}

impl Repository {
    /// 创建确认单，并写入 `CREATED` 审计事件。
    pub async fn create_confirmation(
        &self,
        confirmation: &NewConfirmation,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(map_db_error)?;

        sqlx::query(
            "INSERT INTO intent_confirmations
                 (id, user_id, session_id, intent_summary, risk_level,
                  proposed_changes, token_hash, status, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'PENDING', ?, ?)",
        )
        .bind(&confirmation.id)
        .bind(&confirmation.user_id)
        .bind(&confirmation.session_id)
        .bind(&confirmation.intent_summary)
        .bind(risk_level_text(confirmation.risk_level))
        .bind(&confirmation.proposed_changes)
        .bind(&confirmation.token_hash)
        .bind(confirmation.created_at)
        .bind(confirmation.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(map_db_error)?;

        insert_event(
            &mut *tx,
            &confirmation.id,
            ConfirmationEventType::Created,
            Some(&confirmation.user_id),
            Some(&format!(
                "risk={}, expires_at={}",
                risk_level_text(confirmation.risk_level),
                confirmation.expires_at.to_rfc3339()
            )),
            confirmation.created_at,
        )
        .await?;

        tx.commit().await.map_err(map_db_error)?;
        Ok(())
    }

    /// 按编号查询确认单。
    pub async fn find_confirmation(
        &self,
        confirmation_id: &str,
    ) -> Result<Option<ConfirmationRecord>, RepositoryError> {
        let row: Option<ConfirmationRow> = sqlx::query_as(
            "SELECT id, user_id, session_id, intent_summary, risk_level,
                    proposed_changes, token_hash, status, created_at, expires_at,
                    decided_at, executed_at, result_payload, failure_reason
             FROM intent_confirmations
             WHERE id = ?",
        )
        .bind(confirmation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(row.map(ConfirmationRow::into_record))
    }

    /// 原子抢占确认单：`PENDING -> EXECUTING`。
    ///
    /// 返回 `true` 表示当前调用者获得唯一执行权；`false` 表示已被使用或已过期。
    pub async fn claim_confirmation(
        &self,
        confirmation_id: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "UPDATE intent_confirmations
             SET status = 'EXECUTING', decided_at = ?
             WHERE id = ? AND status = 'PENDING' AND expires_at > ?",
        )
        .bind(now)
        .bind(confirmation_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(result.rows_affected() == 1)
    }

    /// 原子拒绝：`PENDING -> REJECTED`（仅未过期时）。
    pub async fn reject_confirmation(
        &self,
        confirmation_id: &str,
        now: DateTime<Utc>,
        reason: Option<&str>,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "UPDATE intent_confirmations
             SET status = 'REJECTED', decided_at = ?, failure_reason = ?
             WHERE id = ? AND status = 'PENDING' AND expires_at > ?",
        )
        .bind(now)
        .bind(reason)
        .bind(confirmation_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(result.rows_affected() == 1)
    }

    /// 原子标记过期：`PENDING -> EXPIRED`（仅已过期时）。
    pub async fn expire_confirmation(
        &self,
        confirmation_id: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "UPDATE intent_confirmations
             SET status = 'EXPIRED', decided_at = ?
             WHERE id = ? AND status = 'PENDING' AND expires_at <= ?",
        )
        .bind(now)
        .bind(confirmation_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(result.rows_affected() == 1)
    }

    /// 完成执行：`EXECUTING -> EXECUTED / FAILED`。
    pub async fn finish_confirmation(
        &self,
        confirmation_id: &str,
        status: ConfirmationStatus,
        result_payload: Option<&str>,
        failure_reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "UPDATE intent_confirmations
             SET status = ?, executed_at = ?, result_payload = ?, failure_reason = ?
             WHERE id = ? AND status = 'EXECUTING'",
        )
        .bind(status.as_str())
        .bind(now)
        .bind(result_payload)
        .bind(failure_reason)
        .bind(confirmation_id)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(result.rows_affected() == 1)
    }

    /// 追加确认单审计事件。
    pub async fn append_confirmation_event(
        &self,
        confirmation_id: &str,
        event_type: ConfirmationEventType,
        actor_user_id: Option<&str>,
        detail: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        insert_event(
            &self.pool,
            confirmation_id,
            event_type,
            actor_user_id,
            detail,
            now,
        )
        .await
    }

    /// 查询确认单审计事件（按时间升序）。
    pub async fn list_confirmation_events(
        &self,
        confirmation_id: &str,
    ) -> Result<Vec<ConfirmationEvent>, RepositoryError> {
        let rows: Vec<ConfirmationEventRow> = sqlx::query_as(
            "SELECT id, confirmation_id, event_type, actor_user_id, detail, created_at
             FROM intent_confirmation_events
             WHERE confirmation_id = ?
             ORDER BY created_at ASC, rowid ASC",
        )
        .bind(confirmation_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(rows
            .into_iter()
            .map(ConfirmationEventRow::into_event)
            .collect())
    }

    /// 批量把过期确认单标记为 `EXPIRED`，返回处理数量。
    pub async fn expire_stale_confirmations(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        let ids: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM intent_confirmations
             WHERE status = 'PENDING' AND expires_at <= ?
             ORDER BY expires_at ASC
             LIMIT ?",
        )
        .bind(now)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;

        let mut expired = 0usize;
        for (id,) in ids {
            if self.expire_confirmation(&id, now).await? {
                self.append_confirmation_event(
                    &id,
                    ConfirmationEventType::Expired,
                    None,
                    Some("confirmation expired before user decision"),
                    now,
                )
                .await?;
                expired += 1;
            }
        }
        Ok(expired)
    }
}

async fn insert_event<'e, E>(
    executor: E,
    confirmation_id: &str,
    event_type: ConfirmationEventType,
    actor_user_id: Option<&str>,
    detail: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), RepositoryError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO intent_confirmation_events
             (id, confirmation_id, event_type, actor_user_id, detail, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id("CEV"))
    .bind(confirmation_id)
    .bind(event_type.as_str())
    .bind(actor_user_id)
    .bind(detail)
    .bind(now)
    .execute(executor)
    .await
    .map_err(map_db_error)?;
    Ok(())
}
