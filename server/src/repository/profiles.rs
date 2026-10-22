//! 用户画像数据访问（REQ-202610-1176）。
//!
//! 画像本体存于 `user_profiles`；行为样本从 1174 / 1175 的埋点表只读查询。
//! 所有查询均使用绑定参数，不拼接用户输入。

use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::profile::{PreferenceVector, SurveySample, TelemetrySample, UserProfile};

use super::{map_db_error, Repository, RepositoryError};

/// `user_profiles` 行。
#[derive(Debug, FromRow)]
struct UserProfileRow {
    user_id: String,
    font_scale: f64,
    density: f64,
    theme_score: f64,
    radius: f64,
    spacing: f64,
    confidence: f64,
    sample_count: i64,
    version: i64,
    cold_start: i64,
    created_at: DateTime<Utc>,
    updated_at: Option<DateTime<Utc>>,
    last_observed_at: Option<DateTime<Utc>>,
}

impl UserProfileRow {
    fn into_profile(self) -> UserProfile {
        UserProfile {
            user_id: self.user_id,
            preferences: PreferenceVector {
                font_scale: self.font_scale,
                density: self.density,
                theme_score: self.theme_score,
                radius: self.radius,
                spacing: self.spacing,
            },
            confidence: self.confidence,
            sample_count: self.sample_count.max(0) as u64,
            version: self.version.max(1) as u64,
            cold_start: self.cold_start != 0,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_observed_at: self.last_observed_at,
        }
    }
}

/// 1174 埋点事件行。
#[derive(Debug, FromRow)]
struct TelemetrySampleRow {
    dwell_time: i64,
    element_click_rate: f64,
    misclick_rate: f64,
    scroll_depth: f64,
    occurred_at: DateTime<Utc>,
}

impl TelemetrySampleRow {
    fn into_sample(self) -> TelemetrySample {
        TelemetrySample {
            dwell_time_ms: self.dwell_time,
            element_click_rate: self.element_click_rate,
            misclick_rate: self.misclick_rate,
            scroll_depth: self.scroll_depth,
            occurred_at: self.occurred_at,
        }
    }
}

/// 1175 偏好调研行。
#[derive(Debug, FromRow)]
struct SurveySampleRow {
    selected_style_id: String,
    rejected_style_id: String,
    occurred_at: DateTime<Utc>,
}

impl SurveySampleRow {
    fn into_sample(self) -> SurveySample {
        SurveySample {
            selected_style_id: self.selected_style_id,
            rejected_style_id: self.rejected_style_id,
            occurred_at: self.occurred_at,
        }
    }
}

impl Repository {
    /// 查询用户画像；不存在时返回 `None`。
    pub async fn find_user_profile(
        &self,
        user_id: &str,
    ) -> Result<Option<UserProfile>, RepositoryError> {
        let row: Option<UserProfileRow> = sqlx::query_as(
            "SELECT user_id, font_scale, density, theme_score, radius, spacing,
                    confidence, sample_count, version, cold_start,
                    created_at, updated_at, last_observed_at
             FROM user_profiles
             WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(row.map(UserProfileRow::into_profile))
    }

    /// 写入 / 覆盖用户画像。
    pub async fn upsert_user_profile(&self, profile: &UserProfile) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO user_profiles
                 (user_id, font_scale, density, theme_score, radius, spacing,
                  confidence, sample_count, version, cold_start,
                  created_at, updated_at, last_observed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET
                 font_scale = excluded.font_scale,
                 density = excluded.density,
                 theme_score = excluded.theme_score,
                 radius = excluded.radius,
                 spacing = excluded.spacing,
                 confidence = excluded.confidence,
                 sample_count = excluded.sample_count,
                 version = excluded.version,
                 cold_start = excluded.cold_start,
                 updated_at = excluded.updated_at,
                 last_observed_at = excluded.last_observed_at",
        )
        .bind(&profile.user_id)
        .bind(profile.preferences.font_scale)
        .bind(profile.preferences.density)
        .bind(profile.preferences.theme_score)
        .bind(profile.preferences.radius)
        .bind(profile.preferences.spacing)
        .bind(profile.confidence)
        .bind(profile.sample_count as i64)
        .bind(profile.version as i64)
        .bind(if profile.cold_start { 1_i64 } else { 0_i64 })
        .bind(profile.created_at)
        .bind(profile.updated_at)
        .bind(profile.last_observed_at)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(())
    }

    /// 读取用户最近的埋点事件（按发生时间倒序）。
    ///
    /// 依赖 REQ-202610-1174 的 `telemetry_events` 表；若该需求尚未合并 / 建表，
    /// 记录告警并返回空集合，让画像接口保持冷启动而不是 500。
    pub async fn list_profile_telemetry_samples(
        &self,
        user_id: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<TelemetrySample>, RepositoryError> {
        let rows: Result<Vec<TelemetrySampleRow>, sqlx::Error> = sqlx::query_as(
            "SELECT dwell_time, element_click_rate, misclick_rate, scroll_depth, occurred_at
             FROM telemetry_events
             WHERE user_id = ?
               AND datetime(occurred_at) >= datetime(?)
             ORDER BY datetime(occurred_at) DESC
             LIMIT ?",
        )
        .bind(user_id)
        .bind(since)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await;

        match rows {
            Ok(rows) => Ok(rows
                .into_iter()
                .map(TelemetrySampleRow::into_sample)
                .collect()),
            Err(error) if is_missing_table(&error) => {
                tracing::warn!(
                    table = "telemetry_events",
                    "telemetry table is missing; profile refresh falls back to cold start"
                );
                Ok(Vec::new())
            }
            Err(error) => Err(map_db_error(error)),
        }
    }

    /// 读取用户最近的偏好调研（按发生时间倒序）。
    ///
    /// 依赖 REQ-202610-1175 的 `telemetry_surveys` 表；缺失时与埋点表同样降级为空。
    pub async fn list_profile_survey_samples(
        &self,
        user_id: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<SurveySample>, RepositoryError> {
        let rows: Result<Vec<SurveySampleRow>, sqlx::Error> = sqlx::query_as(
            "SELECT selected_style_id, rejected_style_id, occurred_at
             FROM telemetry_surveys
             WHERE user_id = ?
               AND datetime(occurred_at) >= datetime(?)
             ORDER BY datetime(occurred_at) DESC
             LIMIT ?",
        )
        .bind(user_id)
        .bind(since)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await;

        match rows {
            Ok(rows) => Ok(rows.into_iter().map(SurveySampleRow::into_sample).collect()),
            Err(error) if is_missing_table(&error) => {
                tracing::warn!(
                    table = "telemetry_surveys",
                    "survey table is missing; profile refresh falls back to cold start"
                );
                Ok(Vec::new())
            }
            Err(error) => Err(map_db_error(error)),
        }
    }
}

fn is_missing_table(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database_error)
            if database_error.message().to_ascii_lowercase().contains("no such table")
    )
}
