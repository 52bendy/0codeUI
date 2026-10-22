//! 偏好对端点（REQ-202610-1188）。
//!
//! - `POST /api/v1/preference/pairs`：通用入库（隐式指标 / A/B / 人工）；
//! - `POST /api/v1/preference/pairs/from-survey`：由显式调研生成偏好对；
//! - `GET  /api/v1/preference/pairs`：管理端分页查询；
//! - `GET  /api/v1/preference/pairs/{id}`：管理端明细；
//! - `POST /api/v1/preference/pairs/{id}/review`：人工审核；
//! - `GET  /api/v1/preference/stats`：统计；
//! - `GET  /api/v1/preference/sample`：确定性分层抽样（人工检查）；
//! - `POST /api/v1/preference/export`：导出 DPO JSONL（响应内含记录与清单）。
//!
//! 入库接口按鉴权用户隔离；管理接口需 `X-Preference-Token`（生产环境必配）。

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};

use crate::config::Environment;
use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::preference::model::{
    ExportInput, ExportOptions, FromSurveyInput, PairFilter, PairInput, PairPage, PairStatus,
    PreferenceSource, PreferenceStats, ReviewInput, SampleResult,
};
use crate::preference::service::PreferenceService;
use crate::repository::Repository;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::telemetry_survey::{validate_identifier, MAX_IDENTIFIER_LEN};
use crate::validation::ValidatedJson;

/// 列表默认页大小。
const DEFAULT_LIMIT: i64 = 50;
/// 列表最大页大小。
const MAX_LIMIT: i64 = 200;
/// 抽样默认条数。
const DEFAULT_SAMPLE_LIMIT: usize = 20;
/// 抽样最大条数。
const MAX_SAMPLE_LIMIT: usize = 200;

/// `POST /api/v1/preference/pairs`
pub async fn ingest_pair(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedJson(payload): ValidatedJson<PairInput>,
) -> Result<Json<ApiResponse<crate::preference::model::IngestOutcome>>, AppError> {
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    let outcome = service.ingest(&user.user_id, payload.into_draft()).await?;
    Ok(Json(ApiResponse::success(outcome)))
}

/// `POST /api/v1/preference/pairs/from-survey`
pub async fn ingest_from_survey(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedJson(payload): ValidatedJson<FromSurveyInput>,
) -> Result<Json<ApiResponse<crate::preference::model::IngestOutcome>>, AppError> {
    validate_identifier("survey_id", &payload.survey_id, 64).map_err(AppError::invalid_param)?;
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    let outcome = service.from_survey(&user.user_id, payload).await?;
    Ok(Json(ApiResponse::success(outcome)))
}

/// `GET /api/v1/preference/pairs`
pub async fn list_pairs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<PairPage>>, AppError> {
    ensure_admin(&state, &headers)?;
    let filter = parse_filter(&params)?;
    let (limit, offset) = parse_pagination(&params)?;
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    let page = service.list(&filter, limit, offset).await?;
    Ok(Json(ApiResponse::success(page)))
}

/// `GET /api/v1/preference/pairs/{id}`
pub async fn get_pair(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::preference::model::PreferencePair>>, AppError> {
    ensure_admin(&state, &headers)?;
    validate_identifier("id", &id, MAX_IDENTIFIER_LEN).map_err(AppError::invalid_param)?;
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    Ok(Json(ApiResponse::success(service.get(&id).await?)))
}

/// `POST /api/v1/preference/pairs/{id}/review`
pub async fn review_pair(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<String>,
    ValidatedJson(payload): ValidatedJson<ReviewInput>,
) -> Result<Json<ApiResponse<crate::preference::model::PreferencePair>>, AppError> {
    ensure_admin(&state, &headers)?;
    validate_identifier("id", &id, MAX_IDENTIFIER_LEN).map_err(AppError::invalid_param)?;
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    let pair = service
        .review(&id, &user.user_id, payload.decision, payload.note)
        .await?;
    Ok(Json(ApiResponse::success(pair)))
}

/// `GET /api/v1/preference/stats`
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<PreferenceStats>>, AppError> {
    ensure_admin(&state, &headers)?;
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    Ok(Json(ApiResponse::success(service.stats().await?)))
}

/// `GET /api/v1/preference/sample`
pub async fn sample(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<SampleResult>>, AppError> {
    ensure_admin(&state, &headers)?;
    let filter = parse_filter(&params)?;
    let limit = match params.get("limit") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| AppError::invalid_param("limit must be a positive integer"))?,
        None => DEFAULT_SAMPLE_LIMIT,
    };
    if !(1..=MAX_SAMPLE_LIMIT).contains(&limit) {
        return Err(AppError::invalid_param(format!(
            "limit must be within 1..={MAX_SAMPLE_LIMIT}"
        )));
    }
    let seed = params
        .get("seed")
        .cloned()
        .unwrap_or_else(|| "default".to_string());
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    Ok(Json(ApiResponse::success(
        service.sample(&filter, limit, &seed).await?,
    )))
}

/// `POST /api/v1/preference/export`
pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(payload): ValidatedJson<ExportInput>,
) -> Result<Json<ApiResponse<crate::preference::model::ExportBundle>>, AppError> {
    ensure_admin(&state, &headers)?;
    let config = &state.config.preference;
    let options = ExportOptions {
        limit: payload.limit.unwrap_or(config.export_max_records),
        min_quality: payload.min_quality.unwrap_or(config.min_quality),
        max_per_user: payload.max_per_user.unwrap_or(config.export_max_per_user),
        max_per_scene: payload.max_per_scene.unwrap_or(config.export_max_per_scene),
        scene_code: payload.scene_code,
        source: payload.source.map(|source| source.as_str().to_string()),
        include_pending: payload.include_pending,
        dry_run: payload.dry_run,
    };
    let repository = Repository::from_db(&state.db);
    let service = PreferenceService::new(&repository, &state.config.preference);
    let bundle = service.export(&options).await?;
    Ok(Json(ApiResponse::success(bundle)))
}

/// 管理端令牌校验。
fn ensure_admin(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let Some(expected) = state.config.preference.admin_token.as_deref() else {
        return if state.config.env == Environment::Dev {
            Ok(())
        } else {
            Err(AppError::forbidden())
        };
    };
    let provided = headers
        .get("x-preference-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(AppError::forbidden())
    }
}

/// 常量时间比较，避免令牌按字节泄露。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left_byte, right_byte) in left.iter().zip(right) {
        difference |= left_byte ^ right_byte;
    }
    difference == 0
}

/// 解析过滤条件。
fn parse_filter(params: &HashMap<String, String>) -> Result<PairFilter, AppError> {
    let mut filter = PairFilter::default();
    if let Some(value) = params.get("status") {
        let status = PairStatus::parse(value)
            .ok_or_else(|| AppError::invalid_param("status is not a valid pair status"))?;
        filter.statuses.push(status.as_str().to_string());
    }
    if let Some(value) = params.get("source") {
        let source = PreferenceSource::parse(value)
            .ok_or_else(|| AppError::invalid_param("source is not a valid preference source"))?;
        filter.source = Some(source.as_str().to_string());
    }
    if let Some(value) = params.get("scene_code") {
        validate_identifier("scene_code", value, MAX_IDENTIFIER_LEN)
            .map_err(AppError::invalid_param)?;
        filter.scene_code = Some(value.clone());
    }
    if let Some(value) = params.get("user_id") {
        validate_identifier("user_id", value, 64).map_err(AppError::invalid_param)?;
        filter.user_id = Some(value.clone());
    }
    if let Some(value) = params.get("min_quality") {
        let quality = value
            .parse::<f64>()
            .map_err(|_| AppError::invalid_param("min_quality must be a number"))?;
        if !(0.0..=1.0).contains(&quality) {
            return Err(AppError::invalid_param("min_quality must be in [0, 1]"));
        }
        filter.min_quality = Some(quality);
    }
    filter.from = parse_time(params, "from")?;
    filter.to = parse_time(params, "to")?;
    if let (Some(from), Some(to)) = (filter.from, filter.to) {
        if from > to {
            return Err(AppError::invalid_param(
                "from must be earlier than or equal to to",
            ));
        }
    }
    Ok(filter)
}

/// 解析分页参数。
fn parse_pagination(params: &HashMap<String, String>) -> Result<(i64, i64), AppError> {
    let limit = match params.get("limit") {
        Some(value) => value
            .parse::<i64>()
            .map_err(|_| AppError::invalid_param("limit must be an integer"))?,
        None => DEFAULT_LIMIT,
    };
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(AppError::invalid_param(format!(
            "limit must be within 1..={MAX_LIMIT}"
        )));
    }
    let offset = match params.get("offset") {
        Some(value) => value
            .parse::<i64>()
            .map_err(|_| AppError::invalid_param("offset must be an integer"))?,
        None => 0,
    };
    if offset < 0 {
        return Err(AppError::invalid_param("offset must be >= 0"));
    }
    Ok((limit, offset))
}

/// 解析可选 RFC3339 时间。
fn parse_time(
    params: &HashMap<String, String>,
    key: &str,
) -> Result<Option<DateTime<Utc>>, AppError> {
    params
        .get(key)
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|timestamp| timestamp.with_timezone(&Utc))
                .map_err(|_| AppError::invalid_param(format!("{key} must be an RFC3339 timestamp")))
        })
        .transpose()
}
