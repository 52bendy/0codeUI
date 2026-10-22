//! 显式偏好调研端点（REQ-202610-1175，PRD §3.2）。
//!
//! - `POST /api/v1/telemetry/survey`：提交一次 A/B 偏好反馈；
//! - `GET  /api/v1/telemetry/surveys`：按用户维度查询明细（可过滤会话 / 场景 /
//!   样式 / 时间，分页）；
//! - `GET  /api/v1/telemetry/surveys/aggregate`：按样式 / 场景 / A/B 样式对聚合，
//!   供画像引擎（REQ-202610-1176）与 DPO 数据管道（REQ-202610-1188）消费。
//!
//! 所有端点均需鉴权；用户只能读写自己的调研数据（`user_id` 与鉴权用户不一致返回 403）。

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::repository::{new_id, Repository};
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::telemetry_survey::{
    validate_identifier, validate_timestamp, SurveyAggregate, SurveyFilter, SurveyInput,
    SurveyPage, SurveySubmitResult, TelemetrySurvey, MAX_IDENTIFIER_LEN, MAX_SURVEY_ID_LEN,
    MAX_USER_ID_LEN,
};
use crate::validation::ValidatedJson;

/// 明细查询默认页大小。
const DEFAULT_LIMIT: i64 = 50;
/// 明细查询最大页大小。
const MAX_LIMIT: i64 = 200;

/// `POST /api/v1/telemetry/survey`
///
/// 请求示例：
/// ```json
/// {
///   "user_id": "u100",
///   "session_id": "sess_001",
///   "scene_code": "ORDER_DETAIL_PAGE",
///   "selected_style_id": "style_modern_dark",
///   "rejected_style_id": "style_classic_light",
///   "survey_id": "survey_001",
///   "timestamp": "2026-10-22T06:00:00Z"
/// }
/// ```
pub async fn submit_survey(
    State(state): State<AppState>,
    user: AuthUser,
    ValidatedJson(payload): ValidatedJson<SurveyInput>,
) -> Result<Json<ApiResponse<SurveySubmitResult>>, AppError> {
    let now = Utc::now();
    let user_id = payload.user_id.unwrap_or_else(|| user.user_id.clone());
    if user_id != user.user_id {
        return Err(AppError::forbidden());
    }
    validate_identifier("user_id", &user_id, MAX_USER_ID_LEN).map_err(AppError::invalid_param)?;

    if let Some(session_id) = &payload.session_id {
        validate_identifier("session_id", session_id, MAX_IDENTIFIER_LEN)
            .map_err(AppError::invalid_param)?;
    }
    validate_identifier("scene_code", &payload.scene_code, MAX_IDENTIFIER_LEN)
        .map_err(AppError::invalid_param)?;
    validate_identifier(
        "selected_style_id",
        &payload.selected_style_id,
        MAX_IDENTIFIER_LEN,
    )
    .map_err(AppError::invalid_param)?;
    validate_identifier(
        "rejected_style_id",
        &payload.rejected_style_id,
        MAX_IDENTIFIER_LEN,
    )
    .map_err(AppError::invalid_param)?;

    if payload.selected_style_id == payload.rejected_style_id {
        return Err(AppError::invalid_param(
            "selected_style_id and rejected_style_id must differ",
        ));
    }

    let survey_id = match payload.survey_id {
        Some(value) => {
            validate_identifier("survey_id", &value, MAX_SURVEY_ID_LEN)
                .map_err(AppError::invalid_param)?;
            value
        }
        None => new_id("SVY"),
    };

    let timestamp = payload.timestamp.unwrap_or(now);
    validate_timestamp(timestamp, now).map_err(AppError::invalid_param)?;

    let survey = TelemetrySurvey {
        id: new_id("SURV"),
        survey_id,
        user_id,
        session_id: payload.session_id,
        scene_code: payload.scene_code,
        selected_style_id: payload.selected_style_id,
        rejected_style_id: payload.rejected_style_id,
        timestamp,
        received_at: now,
    };

    let repository = Repository::from_db(&state.db);
    let result = repository.insert_telemetry_survey(&survey).await?;
    Ok(Json(ApiResponse::success(result)))
}

/// `GET /api/v1/telemetry/surveys?user_id=&session_id=&scene_code=&style_id=&from=&to=&limit=&offset=`
pub async fn list_surveys(
    State(state): State<AppState>,
    user: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<SurveyPage>>, AppError> {
    let user_id = resolve_query_user(&params, &user.user_id)?;
    let filter = parse_filter(&params)?;
    let (limit, offset) = parse_pagination(&params)?;

    let repository = Repository::from_db(&state.db);
    let (items, total) = repository
        .list_telemetry_surveys(&user_id, &filter, limit, offset)
        .await?;

    Ok(Json(ApiResponse::success(SurveyPage {
        items,
        total,
        limit,
        offset,
    })))
}

/// `GET /api/v1/telemetry/surveys/aggregate?user_id=&scene_code=&from=&to=`
pub async fn aggregate_surveys(
    State(state): State<AppState>,
    user: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<SurveyAggregate>>, AppError> {
    let user_id = resolve_query_user(&params, &user.user_id)?;
    let mut filter = parse_filter(&params)?;
    // 样式过滤只对明细有意义；聚合按全部样式统计，忽略该参数。
    filter.style_id = None;

    let repository = Repository::from_db(&state.db);
    let aggregate = repository
        .aggregate_telemetry_surveys(&user_id, &filter)
        .await?;

    Ok(Json(ApiResponse::success(aggregate)))
}

/// 查询用户：缺省取鉴权用户，显式指定时必须是本人。
fn resolve_query_user(
    params: &HashMap<String, String>,
    auth_user_id: &str,
) -> Result<String, AppError> {
    match params.get("user_id") {
        Some(value) if value != auth_user_id => Err(AppError::forbidden()),
        Some(value) => {
            validate_identifier("user_id", value, MAX_USER_ID_LEN)
                .map_err(AppError::invalid_param)?;
            Ok(value.clone())
        }
        None => Ok(auth_user_id.to_string()),
    }
}

/// 解析会话 / 场景 / 样式 / 时间过滤条件。
fn parse_filter(params: &HashMap<String, String>) -> Result<SurveyFilter, AppError> {
    let session_id = parse_identifier(params, "session_id")?;
    let scene_code = parse_identifier(params, "scene_code")?;
    let style_id = parse_identifier(params, "style_id")?;
    let from = parse_time(params, "from")?;
    let to = parse_time(params, "to")?;

    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err(AppError::invalid_param(
                "from must be earlier than or equal to to",
            ));
        }
    }

    Ok(SurveyFilter {
        session_id,
        scene_code,
        style_id,
        from,
        to,
    })
}

/// 解析并校验分页参数。
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

/// 解析可选标识符参数（校验字符集与长度）。
fn parse_identifier(
    params: &HashMap<String, String>,
    key: &str,
) -> Result<Option<String>, AppError> {
    match params.get(key) {
        Some(value) => {
            validate_identifier(key, value, MAX_IDENTIFIER_LEN).map_err(AppError::invalid_param)?;
            Ok(Some(value.clone()))
        }
        None => Ok(None),
    }
}

/// 解析可选 RFC3339 时间参数。
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
