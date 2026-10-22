//! 偏好对数据访问（REQ-202610-1188）。
//!
//! 所有查询使用绑定参数（`QueryBuilder`），不拼接用户输入；幂等写入依赖
//! `pair_key` 唯一约束。

use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{QueryBuilder, Sqlite};

use crate::preference::model::{
    CountBucket, IngestOutcome, PairFilter, PreferenceExportRecord, PreferencePair, PreferenceStats,
};

use super::{map_db_error, Repository, RepositoryError};

impl Repository {
    /// 写入偏好对；`pair_key` 冲突时返回已存在记录。
    pub async fn insert_preference_pair(
        &self,
        pair: &PreferencePair,
    ) -> Result<IngestOutcome, RepositoryError> {
        let quality_flags =
            serde_json::to_string(&pair.quality_flags).unwrap_or_else(|_| "[]".into());
        let metadata = pair.metadata.to_string();
        let result = sqlx::query(
            "INSERT INTO preference_pairs
                 (id, pair_key, content_hash, prompt, chosen_html, rejected_html,
                  source, scene_code, user_id, session_id, request_id, survey_id,
                  prompt_template_version, chosen_model, rejected_model,
                  chosen_style_id, rejected_style_id, pipeline_version,
                  preference_strength, quality_score, quality_flags, metadata,
                  status, review_note, reviewed_by, reviewed_at, exported_at,
                  occurred_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(pair_key) DO NOTHING",
        )
        .bind(&pair.id)
        .bind(&pair.pair_key)
        .bind(&pair.content_hash)
        .bind(&pair.prompt)
        .bind(&pair.chosen_html)
        .bind(&pair.rejected_html)
        .bind(&pair.source)
        .bind(&pair.scene_code)
        .bind(&pair.user_id)
        .bind(&pair.session_id)
        .bind(&pair.request_id)
        .bind(&pair.survey_id)
        .bind(&pair.prompt_template_version)
        .bind(&pair.chosen_model)
        .bind(&pair.rejected_model)
        .bind(&pair.chosen_style_id)
        .bind(&pair.rejected_style_id)
        .bind(&pair.pipeline_version)
        .bind(pair.preference_strength)
        .bind(pair.quality_score)
        .bind(quality_flags)
        .bind(metadata)
        .bind(&pair.status)
        .bind(&pair.review_note)
        .bind(&pair.reviewed_by)
        .bind(pair.reviewed_at)
        .bind(pair.exported_at)
        .bind(pair.occurred_at)
        .bind(pair.created_at)
        .bind(pair.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        if result.rows_affected() == 1 {
            return Ok(IngestOutcome {
                accepted: true,
                duplicate: false,
                pair: pair.clone(),
            });
        }

        let existing = self
            .find_preference_pair_by_key(&pair.pair_key)
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict(format!(
                    "preference pair {} already exists but could not be loaded",
                    pair.pair_key
                ))
            })?;
        Ok(IngestOutcome {
            accepted: false,
            duplicate: true,
            pair: existing,
        })
    }

    /// 按幂等键查询。
    pub async fn find_preference_pair_by_key(
        &self,
        pair_key: &str,
    ) -> Result<Option<PreferencePair>, RepositoryError> {
        let row = sqlx::query_as::<_, PreferencePairRow>(&format!(
            "{SELECT_PREFERENCE_PAIR} WHERE pair_key = ?"
        ))
        .bind(pair_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(row.map(PreferencePairRow::into_pair))
    }

    /// 按主键查询。
    pub async fn get_preference_pair(
        &self,
        id: &str,
    ) -> Result<Option<PreferencePair>, RepositoryError> {
        let row = sqlx::query_as::<_, PreferencePairRow>(&format!(
            "{SELECT_PREFERENCE_PAIR} WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(row.map(PreferencePairRow::into_pair))
    }

    /// 分页查询。
    pub async fn list_preference_pairs(
        &self,
        filter: &PairFilter,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<PreferencePair>, i64), RepositoryError> {
        let mut count_query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM preference_pairs");
        push_pair_filters(&mut count_query, filter);
        let total: i64 = count_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(map_db_error)?;

        let mut query = QueryBuilder::<Sqlite>::new(SELECT_PREFERENCE_PAIR);
        push_pair_filters(&mut query, filter);
        query
            .push(" ORDER BY created_at DESC, id DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows = query
            .build_query_as::<PreferencePairRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(map_db_error)?;
        Ok((
            rows.into_iter().map(PreferencePairRow::into_pair).collect(),
            total,
        ))
    }

    /// 更新审核状态。
    pub async fn update_preference_pair_review(
        &self,
        id: &str,
        status: &str,
        note: Option<&str>,
        reviewer: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<PreferencePair>, RepositoryError> {
        let result = sqlx::query(
            "UPDATE preference_pairs
             SET status = ?, review_note = ?, reviewed_by = ?, reviewed_at = ?, updated_at = ?
             WHERE id = ? AND status <> 'EXPORTED'",
        )
        .bind(status)
        .bind(note)
        .bind(reviewer)
        .bind(now)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_preference_pair(id).await
    }

    /// 汇总统计。
    pub async fn preference_stats(&self) -> Result<PreferenceStats, RepositoryError> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM preference_pairs")
            .fetch_one(&self.pool)
            .await
            .map_err(map_db_error)?;
        let by_status = self
            .count_buckets("SELECT status AS bucket_key, COUNT(*) AS count FROM preference_pairs GROUP BY status ORDER BY count DESC, bucket_key ASC")
            .await?;
        let by_source = self
            .count_buckets("SELECT source AS bucket_key, COUNT(*) AS count FROM preference_pairs GROUP BY source ORDER BY count DESC, bucket_key ASC")
            .await?;
        let by_scene = self
            .count_buckets("SELECT scene_code AS bucket_key, COUNT(*) AS count FROM preference_pairs GROUP BY scene_code ORDER BY count DESC, bucket_key ASC LIMIT 20")
            .await?;
        let average_quality: f64 =
            sqlx::query_scalar("SELECT COALESCE(AVG(quality_score), 0.0) FROM preference_pairs")
                .fetch_one(&self.pool)
                .await
                .map_err(map_db_error)?;
        let exportable: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM preference_pairs WHERE status = 'ACCEPTED'")
                .fetch_one(&self.pool)
                .await
                .map_err(map_db_error)?;
        let low_quality_rejected: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM preference_pairs
             WHERE status = 'REJECTED' AND review_note LIKE 'auto-rejected:%'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)?;
        let last_created_at: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT MAX(created_at) FROM preference_pairs")
                .fetch_one(&self.pool)
                .await
                .map_err(map_db_error)?;

        Ok(PreferenceStats {
            total,
            by_status,
            by_source,
            by_scene,
            average_quality: (average_quality * 10_000.0).round() / 10_000.0,
            exportable,
            low_quality_rejected,
            last_created_at,
        })
    }

    /// 写入导出批次记录。
    pub async fn insert_preference_export(
        &self,
        export: &PreferenceExportRecord,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO preference_exports
                 (id, dataset_id, format, record_count, sha256, filters_json, manifest_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&export.id)
        .bind(&export.dataset_id)
        .bind(&export.format)
        .bind(export.record_count)
        .bind(&export.sha256)
        .bind(&export.filters_json)
        .bind(&export.manifest_json)
        .bind(export.created_at)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(())
    }

    /// 把导出的偏好对标记为 `EXPORTED`。
    pub async fn mark_preference_pairs_exported(
        &self,
        ids: &[String],
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut transaction = self.pool.begin().await.map_err(map_db_error)?;
        for id in ids {
            sqlx::query(
                "UPDATE preference_pairs
                 SET status = 'EXPORTED', exported_at = ?, updated_at = ?
                 WHERE id = ? AND status = 'ACCEPTED'",
            )
            .bind(now)
            .bind(now)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(map_db_error)?;
        }
        transaction.commit().await.map_err(map_db_error)?;
        Ok(())
    }

    /// 执行聚合查询并映射为桶列表。
    async fn count_buckets(&self, sql: &str) -> Result<Vec<CountBucket>, RepositoryError> {
        let rows = sqlx::query_as::<_, CountBucketRow>(sql)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db_error)?;
        Ok(rows
            .into_iter()
            .map(|row| CountBucket {
                key: row.bucket_key,
                count: row.count,
            })
            .collect())
    }
}

const SELECT_PREFERENCE_PAIR: &str = "SELECT id, pair_key, content_hash, prompt, chosen_html, \
    rejected_html, source, scene_code, user_id, session_id, request_id, survey_id, \
    prompt_template_version, chosen_model, rejected_model, chosen_style_id, rejected_style_id, \
    pipeline_version, preference_strength, quality_score, quality_flags AS quality_flags_json, \
    metadata AS metadata_json, status, review_note, reviewed_by, reviewed_at, exported_at, \
    occurred_at, created_at, updated_at FROM preference_pairs";

/// 把过滤条件追加到查询。
fn push_pair_filters(query: &mut QueryBuilder<'_, Sqlite>, filter: &PairFilter) {
    query.push(" WHERE 1 = 1");
    if !filter.statuses.is_empty() {
        query.push(" AND status IN (");
        for (index, status) in filter.statuses.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push_bind(status.clone());
        }
        query.push(")");
    }
    if let Some(source) = &filter.source {
        query.push(" AND source = ").push_bind(source.clone());
    }
    if let Some(scene_code) = &filter.scene_code {
        query
            .push(" AND scene_code = ")
            .push_bind(scene_code.clone());
    }
    if let Some(user_id) = &filter.user_id {
        query.push(" AND user_id = ").push_bind(user_id.clone());
    }
    if let Some(min_quality) = filter.min_quality {
        query.push(" AND quality_score >= ").push_bind(min_quality);
    }
    if let Some(from) = filter.from {
        query.push(" AND created_at >= ").push_bind(from);
    }
    if let Some(to) = filter.to {
        query.push(" AND created_at <= ").push_bind(to);
    }
}

#[derive(Debug, sqlx::FromRow)]
struct PreferencePairRow {
    id: String,
    pair_key: String,
    content_hash: String,
    prompt: String,
    chosen_html: String,
    rejected_html: String,
    source: String,
    scene_code: String,
    user_id: String,
    session_id: Option<String>,
    request_id: Option<String>,
    survey_id: Option<String>,
    prompt_template_version: String,
    chosen_model: Option<String>,
    rejected_model: Option<String>,
    chosen_style_id: Option<String>,
    rejected_style_id: Option<String>,
    pipeline_version: Option<String>,
    preference_strength: f64,
    quality_score: f64,
    quality_flags_json: String,
    metadata_json: String,
    status: String,
    review_note: Option<String>,
    reviewed_by: Option<String>,
    reviewed_at: Option<DateTime<Utc>>,
    exported_at: Option<DateTime<Utc>>,
    occurred_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl PreferencePairRow {
    fn into_pair(self) -> PreferencePair {
        PreferencePair {
            id: self.id,
            pair_key: self.pair_key,
            content_hash: self.content_hash,
            prompt: self.prompt,
            chosen_html: self.chosen_html,
            rejected_html: self.rejected_html,
            source: self.source,
            scene_code: self.scene_code,
            user_id: self.user_id,
            session_id: self.session_id,
            request_id: self.request_id,
            survey_id: self.survey_id,
            prompt_template_version: self.prompt_template_version,
            chosen_model: self.chosen_model,
            rejected_model: self.rejected_model,
            chosen_style_id: self.chosen_style_id,
            rejected_style_id: self.rejected_style_id,
            pipeline_version: self.pipeline_version,
            preference_strength: self.preference_strength,
            quality_score: self.quality_score,
            quality_flags: serde_json::from_str(&self.quality_flags_json).unwrap_or_default(),
            metadata: serde_json::from_str(&self.metadata_json).unwrap_or_else(|_| json!({})),
            status: self.status,
            review_note: self.review_note,
            reviewed_by: self.reviewed_by,
            reviewed_at: self.reviewed_at,
            exported_at: self.exported_at,
            occurred_at: self.occurred_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct CountBucketRow {
    bucket_key: String,
    count: i64,
}
