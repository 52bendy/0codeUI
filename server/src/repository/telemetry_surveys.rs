//! 显式偏好调研数据访问（REQ-202610-1175）。
//!
//! 写入使用唯一约束 `(user_id, survey_id)` 保证幂等；查询全部使用绑定参数
//! （`QueryBuilder`），不拼接用户输入。

use std::collections::HashMap;

use sqlx::{QueryBuilder, Sqlite};

use crate::telemetry_survey::{
    SurveyAggregate, SurveyFilter, SurveyPairStat, SurveySceneStat, SurveyStyleStat,
    SurveySubmitResult, TelemetrySurvey,
};

use super::{map_db_error, Repository, RepositoryError};

impl Repository {
    /// 写入一条调研记录；同一用户下重复 `survey_id` 返回已落库记录。
    pub async fn insert_telemetry_survey(
        &self,
        survey: &TelemetrySurvey,
    ) -> Result<SurveySubmitResult, RepositoryError> {
        let result = sqlx::query(
            "INSERT INTO telemetry_surveys
                 (id, survey_id, user_id, session_id, scene_code,
                  selected_style_id, rejected_style_id, occurred_at, received_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, survey_id) DO NOTHING",
        )
        .bind(&survey.id)
        .bind(&survey.survey_id)
        .bind(&survey.user_id)
        .bind(&survey.session_id)
        .bind(&survey.scene_code)
        .bind(&survey.selected_style_id)
        .bind(&survey.rejected_style_id)
        .bind(survey.timestamp)
        .bind(survey.received_at)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        if result.rows_affected() == 1 {
            return Ok(SurveySubmitResult {
                accepted: true,
                duplicate: false,
                survey: survey.clone(),
            });
        }

        let existing = self
            .find_telemetry_survey(&survey.user_id, &survey.survey_id)
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict(format!(
                    "survey {} already exists but could not be loaded",
                    survey.survey_id
                ))
            })?;

        Ok(SurveySubmitResult {
            accepted: false,
            duplicate: true,
            survey: existing,
        })
    }

    /// 按 `(user_id, survey_id)` 查询单条调研记录。
    pub async fn find_telemetry_survey(
        &self,
        user_id: &str,
        survey_id: &str,
    ) -> Result<Option<TelemetrySurvey>, RepositoryError> {
        let record = sqlx::query_as::<_, TelemetrySurvey>(
            "SELECT id, survey_id, user_id, session_id, scene_code,
                    selected_style_id, rejected_style_id,
                    occurred_at AS timestamp, received_at
             FROM telemetry_surveys
             WHERE user_id = ? AND survey_id = ?",
        )
        .bind(user_id)
        .bind(survey_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;
        Ok(record)
    }

    /// 按用户维度查询调研明细（可按会话 / 场景 / 样式 / 时间过滤），返回当前页与总条数。
    pub async fn list_telemetry_surveys(
        &self,
        user_id: &str,
        filter: &SurveyFilter,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<TelemetrySurvey>, i64), RepositoryError> {
        let mut count_query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM telemetry_surveys");
        push_survey_filters(&mut count_query, user_id, filter);
        let total: i64 = count_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(map_db_error)?;

        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, survey_id, user_id, session_id, scene_code,
                    selected_style_id, rejected_style_id,
                    occurred_at AS timestamp, received_at
             FROM telemetry_surveys",
        );
        push_survey_filters(&mut query, user_id, filter);
        query
            .push(" ORDER BY occurred_at DESC, id DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let items = query
            .build_query_as::<TelemetrySurvey>()
            .fetch_all(&self.pool)
            .await
            .map_err(map_db_error)?;

        Ok((items, total))
    }

    /// 按用户维度聚合调研样本：样式选择 / 拒绝次数、场景分布与 A/B 样式对。
    pub async fn aggregate_telemetry_surveys(
        &self,
        user_id: &str,
        filter: &SurveyFilter,
    ) -> Result<SurveyAggregate, RepositoryError> {
        let mut total_query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM telemetry_surveys");
        push_survey_filters(&mut total_query, user_id, filter);
        let total_surveys: i64 = total_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(map_db_error)?;

        let selected_rows = self.survey_style_counts(user_id, filter, true).await?;
        let rejected_rows = self.survey_style_counts(user_id, filter, false).await?;

        let mut styles: HashMap<String, SurveyStyleStat> = HashMap::new();
        for row in selected_rows {
            styles
                .entry(row.style_id.clone())
                .or_insert_with(|| SurveyStyleStat {
                    style_id: row.style_id,
                    selected_count: 0,
                    rejected_count: 0,
                    net_preference: 0,
                })
                .selected_count = row.style_count;
        }
        for row in rejected_rows {
            styles
                .entry(row.style_id.clone())
                .or_insert_with(|| SurveyStyleStat {
                    style_id: row.style_id,
                    selected_count: 0,
                    rejected_count: 0,
                    net_preference: 0,
                })
                .rejected_count = row.style_count;
        }
        let mut styles: Vec<SurveyStyleStat> = styles.into_values().collect();
        for style in &mut styles {
            style.net_preference = style.selected_count - style.rejected_count;
        }
        styles.sort_by(|left, right| {
            right
                .net_preference
                .cmp(&left.net_preference)
                .then_with(|| right.selected_count.cmp(&left.selected_count))
                .then_with(|| left.style_id.cmp(&right.style_id))
        });

        let mut scenes_query = QueryBuilder::<Sqlite>::new(
            "SELECT scene_code, COUNT(*) AS survey_count
             FROM telemetry_surveys",
        );
        push_survey_filters(&mut scenes_query, user_id, filter);
        scenes_query.push(" GROUP BY scene_code ORDER BY survey_count DESC, scene_code ASC");
        let scenes = scenes_query
            .build_query_as::<SurveySceneStat>()
            .fetch_all(&self.pool)
            .await
            .map_err(map_db_error)?;

        let mut pairs_query = QueryBuilder::<Sqlite>::new(
            "SELECT scene_code, selected_style_id, rejected_style_id,
                    COUNT(*) AS pair_count
             FROM telemetry_surveys",
        );
        push_survey_filters(&mut pairs_query, user_id, filter);
        pairs_query.push(
            " GROUP BY scene_code, selected_style_id, rejected_style_id
              ORDER BY pair_count DESC, scene_code ASC,
                       selected_style_id ASC, rejected_style_id ASC
              LIMIT 100",
        );
        let pair_rows = pairs_query
            .build_query_as::<PairCountRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(map_db_error)?;
        let pairs = pair_rows
            .into_iter()
            .map(|row| SurveyPairStat {
                scene_code: row.scene_code,
                selected_style_id: row.selected_style_id,
                rejected_style_id: row.rejected_style_id,
                count: row.pair_count,
            })
            .collect();

        Ok(SurveyAggregate {
            user_id: user_id.to_string(),
            from: filter.from,
            to: filter.to,
            total_surveys,
            styles,
            scenes,
            pairs,
        })
    }

    /// 统计样式被选择或拒绝的次数。
    async fn survey_style_counts(
        &self,
        user_id: &str,
        filter: &SurveyFilter,
        selected: bool,
    ) -> Result<Vec<StyleCountRow>, RepositoryError> {
        let column = if selected {
            "selected_style_id"
        } else {
            "rejected_style_id"
        };
        let mut query = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {column} AS style_id, COUNT(*) AS style_count
             FROM telemetry_surveys"
        ));
        push_survey_filters(&mut query, user_id, filter);
        query.push(format!(" GROUP BY {column}"));
        query
            .build_query_as::<StyleCountRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(map_db_error)
    }
}

/// 把用户 / 会话 / 场景 / 样式 / 时间过滤条件追加到查询。
fn push_survey_filters(query: &mut QueryBuilder<'_, Sqlite>, user_id: &str, filter: &SurveyFilter) {
    query
        .push(" WHERE user_id = ")
        .push_bind(user_id.to_string());

    if let Some(session_id) = &filter.session_id {
        query
            .push(" AND session_id = ")
            .push_bind(session_id.clone());
    }
    if let Some(scene_code) = &filter.scene_code {
        query
            .push(" AND scene_code = ")
            .push_bind(scene_code.clone());
    }
    if let Some(style_id) = &filter.style_id {
        query
            .push(" AND (selected_style_id = ")
            .push_bind(style_id.clone())
            .push(" OR rejected_style_id = ")
            .push_bind(style_id.clone())
            .push(")");
    }
    if let Some(from) = filter.from {
        query.push(" AND occurred_at >= ").push_bind(from);
    }
    if let Some(to) = filter.to {
        query.push(" AND occurred_at <= ").push_bind(to);
    }
}

#[derive(Debug, sqlx::FromRow)]
struct StyleCountRow {
    style_id: String,
    style_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct PairCountRow {
    scene_code: String,
    selected_style_id: String,
    rejected_style_id: String,
    pair_count: i64,
}
