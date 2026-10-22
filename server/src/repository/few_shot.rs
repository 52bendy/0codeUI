//! Few-Shot 组件片段库的数据访问（REQ-202610-1183）。
//!
//! 片段本体存于 `ui_few_shot_snippets`；06 埋点与 07 调研表只读查询，
//! 依赖表缺失时降级为空集合（与画像模块的降级策略一致），不让检索接口 500。

use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::few_shot::model::{ComponentSnippet, SnippetSource, SnippetStyle, SnippetTheme};
use crate::profile::{SurveySample, TelemetrySample};

use super::{map_db_error, Repository, RepositoryError};

/// `ui_few_shot_snippets` 行。
#[derive(Debug, FromRow)]
struct FewShotSnippetRow {
    id: String,
    source_type: String,
    source_ref: Option<String>,
    scene_code: String,
    title: String,
    snippet_html: String,
    snippet_text: String,
    tags_json: String,
    theme: String,
    font_scale: Option<f64>,
    density: Option<f64>,
    radius: Option<f64>,
    spacing: Option<f64>,
    satisfaction: f64,
    implicit_score: f64,
    usage_count: i64,
    positive_feedback: i64,
    negative_feedback: i64,
    enabled: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FewShotSnippetRow {
    fn into_snippet(self) -> Result<ComponentSnippet, RepositoryError> {
        let source_type = SnippetSource::parse(&self.source_type).ok_or_else(|| {
            RepositoryError::Invalid(format!("unknown snippet source_type: {}", self.source_type))
        })?;
        let theme = SnippetTheme::parse(&self.theme).ok_or_else(|| {
            RepositoryError::Invalid(format!("unknown snippet theme: {}", self.theme))
        })?;
        let tags: Vec<String> = serde_json::from_str(&self.tags_json).map_err(|error| {
            RepositoryError::Invalid(format!("invalid snippet tags_json: {error}"))
        })?;

        Ok(ComponentSnippet {
            id: self.id,
            source_type,
            source_ref: self.source_ref,
            scene_code: self.scene_code,
            title: self.title,
            snippet_html: self.snippet_html,
            snippet_text: self.snippet_text,
            tags,
            style: SnippetStyle {
                theme,
                font_scale: self.font_scale,
                density: self.density,
                radius: self.radius,
                spacing: self.spacing,
            },
            satisfaction: self.satisfaction,
            implicit_score: self.implicit_score,
            usage_count: self.usage_count.max(0) as u64,
            positive_feedback: self.positive_feedback.max(0) as u64,
            negative_feedback: self.negative_feedback.max(0) as u64,
            enabled: self.enabled != 0,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const SNIPPET_COLUMNS: &str = "id, source_type, source_ref, scene_code, title, snippet_html, \
     snippet_text, tags_json, theme, font_scale, density, radius, spacing, \
     satisfaction, implicit_score, usage_count, positive_feedback, negative_feedback, \
     enabled, created_at, updated_at";

impl Repository {
    /// 按 ID 查询片段。
    pub async fn find_few_shot_snippet(
        &self,
        id: &str,
    ) -> Result<Option<ComponentSnippet>, RepositoryError> {
        let row: Option<FewShotSnippetRow> = sqlx::query_as(&format!(
            "SELECT {SNIPPET_COLUMNS} FROM ui_few_shot_snippets WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        row.map(FewShotSnippetRow::into_snippet).transpose()
    }

    /// 按来源引用查询片段（生成管线幂等入库）。
    pub async fn find_few_shot_snippet_by_source(
        &self,
        source_type: SnippetSource,
        source_ref: &str,
    ) -> Result<Option<ComponentSnippet>, RepositoryError> {
        let row: Option<FewShotSnippetRow> = sqlx::query_as(&format!(
            "SELECT {SNIPPET_COLUMNS} FROM ui_few_shot_snippets
             WHERE source_type = ? AND source_ref = ?"
        ))
        .bind(source_type.as_str())
        .bind(source_ref)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        row.map(FewShotSnippetRow::into_snippet).transpose()
    }

    /// 插入或按主键更新片段。
    pub async fn upsert_few_shot_snippet(
        &self,
        snippet: &ComponentSnippet,
    ) -> Result<(), RepositoryError> {
        let tags_json = serde_json::to_string(&snippet.tags)
            .map_err(|error| RepositoryError::Invalid(format!("invalid tags: {error}")))?;

        sqlx::query(
            "INSERT INTO ui_few_shot_snippets
                 (id, source_type, source_ref, scene_code, title, snippet_html, snippet_text,
                  tags_json, theme, font_scale, density, radius, spacing,
                  satisfaction, implicit_score, usage_count, positive_feedback,
                  negative_feedback, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                 source_type = excluded.source_type,
                 source_ref = excluded.source_ref,
                 scene_code = excluded.scene_code,
                 title = excluded.title,
                 snippet_html = excluded.snippet_html,
                 snippet_text = excluded.snippet_text,
                 tags_json = excluded.tags_json,
                 theme = excluded.theme,
                 font_scale = excluded.font_scale,
                 density = excluded.density,
                 radius = excluded.radius,
                 spacing = excluded.spacing,
                 satisfaction = excluded.satisfaction,
                 implicit_score = excluded.implicit_score,
                 enabled = excluded.enabled,
                 updated_at = excluded.updated_at",
        )
        .bind(&snippet.id)
        .bind(snippet.source_type.as_str())
        .bind(&snippet.source_ref)
        .bind(&snippet.scene_code)
        .bind(&snippet.title)
        .bind(&snippet.snippet_html)
        .bind(&snippet.snippet_text)
        .bind(tags_json)
        .bind(snippet.style.theme.as_str())
        .bind(snippet.style.font_scale)
        .bind(snippet.style.density)
        .bind(snippet.style.radius)
        .bind(snippet.style.spacing)
        .bind(snippet.satisfaction)
        .bind(snippet.implicit_score)
        .bind(snippet.usage_count as i64)
        .bind(snippet.positive_feedback as i64)
        .bind(snippet.negative_feedback as i64)
        .bind(if snippet.enabled { 1_i64 } else { 0_i64 })
        .bind(snippet.created_at)
        .bind(snippet.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(())
    }

    /// 查询场景下参与检索的候选片段。
    pub async fn list_few_shot_candidates(
        &self,
        scene_code: &str,
        limit: i64,
    ) -> Result<Vec<ComponentSnippet>, RepositoryError> {
        let rows: Vec<FewShotSnippetRow> = sqlx::query_as(&format!(
            "SELECT {SNIPPET_COLUMNS} FROM ui_few_shot_snippets
             WHERE scene_code = ? AND enabled = 1
             ORDER BY satisfaction DESC, updated_at DESC, id ASC
             LIMIT ?"
        ))
        .bind(scene_code)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;

        rows.into_iter()
            .map(FewShotSnippetRow::into_snippet)
            .collect()
    }

    /// 分页列出片段（可按场景过滤）。
    pub async fn list_few_shot_snippets(
        &self,
        scene_code: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<ComponentSnippet>, i64), RepositoryError> {
        let (rows, total): (Vec<FewShotSnippetRow>, i64) = match scene_code {
            Some(scene) => {
                let rows = sqlx::query_as(&format!(
                    "SELECT {SNIPPET_COLUMNS} FROM ui_few_shot_snippets
                     WHERE scene_code = ?
                     ORDER BY satisfaction DESC, updated_at DESC, id ASC
                     LIMIT ? OFFSET ?"
                ))
                .bind(scene)
                .bind(limit.max(1))
                .bind(offset.max(0))
                .fetch_all(&self.pool)
                .await
                .map_err(map_db_error)?;
                let total: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM ui_few_shot_snippets WHERE scene_code = ?",
                )
                .bind(scene)
                .fetch_one(&self.pool)
                .await
                .map_err(map_db_error)?;
                (rows, total)
            }
            None => {
                let rows = sqlx::query_as(&format!(
                    "SELECT {SNIPPET_COLUMNS} FROM ui_few_shot_snippets
                     ORDER BY satisfaction DESC, updated_at DESC, id ASC
                     LIMIT ? OFFSET ?"
                ))
                .bind(limit.max(1))
                .bind(offset.max(0))
                .fetch_all(&self.pool)
                .await
                .map_err(map_db_error)?;
                let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ui_few_shot_snippets")
                    .fetch_one(&self.pool)
                    .await
                    .map_err(map_db_error)?;
                (rows, total)
            }
        };

        let snippets = rows
            .into_iter()
            .map(FewShotSnippetRow::into_snippet)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((snippets, total))
    }

    /// 记录一次片段满意度反馈。
    ///
    /// 满意度使用 EWMA（新值权重 0.3）；正负反馈计数用于推导隐式质量。
    pub async fn apply_few_shot_feedback(
        &self,
        id: &str,
        satisfaction: f64,
        now: DateTime<Utc>,
    ) -> Result<Option<ComponentSnippet>, RepositoryError> {
        let Some(current) = self.find_few_shot_snippet(id).await? else {
            return Ok(None);
        };

        let next_satisfaction =
            (current.satisfaction * 0.7 + satisfaction.clamp(0.0, 1.0) * 0.3).clamp(0.0, 1.0);
        let positive = current.positive_feedback + u64::from(satisfaction >= 0.6);
        let negative = current.negative_feedback + u64::from(satisfaction < 0.4);
        let implicit =
            ((positive as f64 + 1.0) / ((positive + negative) as f64 + 2.0)).clamp(0.0, 1.0);

        sqlx::query(
            "UPDATE ui_few_shot_snippets
             SET satisfaction = ?, implicit_score = ?, usage_count = usage_count + 1,
                 positive_feedback = ?, negative_feedback = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(next_satisfaction)
        .bind(implicit)
        .bind(positive as i64)
        .bind(negative as i64)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        self.find_few_shot_snippet(id).await
    }

    /// 读取用户在某场景的最近埋点样本（06）。
    pub async fn list_scene_telemetry_samples(
        &self,
        user_id: &str,
        scene_code: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<TelemetrySample>, RepositoryError> {
        let rows: Result<Vec<SceneTelemetryRow>, sqlx::Error> = sqlx::query_as(
            "SELECT dwell_time, element_click_rate, misclick_rate, scroll_depth, occurred_at
             FROM telemetry_events
             WHERE user_id = ? AND page_id = ?
               AND datetime(occurred_at) >= datetime(?)
             ORDER BY datetime(occurred_at) DESC
             LIMIT ?",
        )
        .bind(user_id)
        .bind(scene_code)
        .bind(since)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await;

        match rows {
            Ok(rows) => Ok(rows
                .into_iter()
                .map(SceneTelemetryRow::into_sample)
                .collect()),
            Err(error) if is_missing_table(&error) => {
                tracing::warn!(
                    table = "telemetry_events",
                    "telemetry table is missing; few-shot falls back to neutral engagement"
                );
                Ok(Vec::new())
            }
            Err(error) => Err(map_db_error(error)),
        }
    }

    /// 读取用户在某场景的最近调研样本（07）。
    pub async fn list_scene_survey_samples(
        &self,
        user_id: &str,
        scene_code: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<SurveySample>, RepositoryError> {
        let rows: Result<Vec<SceneSurveyRow>, sqlx::Error> = sqlx::query_as(
            "SELECT selected_style_id, rejected_style_id, occurred_at
             FROM telemetry_surveys
             WHERE user_id = ? AND scene_code = ?
               AND datetime(occurred_at) >= datetime(?)
             ORDER BY datetime(occurred_at) DESC
             LIMIT ?",
        )
        .bind(user_id)
        .bind(scene_code)
        .bind(since)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await;

        match rows {
            Ok(rows) => Ok(rows.into_iter().map(SceneSurveyRow::into_sample).collect()),
            Err(error) if is_missing_table(&error) => {
                tracing::warn!(
                    table = "telemetry_surveys",
                    "survey table is missing; few-shot falls back to profile-only ranking"
                );
                Ok(Vec::new())
            }
            Err(error) => Err(map_db_error(error)),
        }
    }
}

#[derive(Debug, FromRow)]
struct SceneTelemetryRow {
    dwell_time: i64,
    element_click_rate: f64,
    misclick_rate: f64,
    scroll_depth: f64,
    occurred_at: DateTime<Utc>,
}

impl SceneTelemetryRow {
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

#[derive(Debug, FromRow)]
struct SceneSurveyRow {
    selected_style_id: String,
    rejected_style_id: String,
    occurred_at: DateTime<Utc>,
}

impl SceneSurveyRow {
    fn into_sample(self) -> SurveySample {
        SurveySample {
            selected_style_id: self.selected_style_id,
            rejected_style_id: self.rejected_style_id,
            occurred_at: self.occurred_at,
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
