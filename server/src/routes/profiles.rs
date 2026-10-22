//! 用户画像读写端点（REQ-202610-1176）。
//!
//! - `GET  /api/v1/users/{user_id}/profile`：读取画像（冷启动返回默认值）；
//! - `PUT  /api/v1/users/{user_id}/profile`：用户主动设置偏好；
//! - `POST /api/v1/users/{user_id}/profile/refresh`：按 1174 / 1175 样本重算画像。
//!
//! 三个端点都要求 Bearer 鉴权，并校验路径 `user_id` 与凭据归属一致，
//! 防止横向越权读取 / 篡改他人画像。

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::profile::{
    Observation, ProfileEngine, ProfileEngineConfig, ProfileResponse, ProfileUpdate, Theme,
    UpdateSkipReason, UserProfile, DENSITY_MAX, DENSITY_MIN, FONT_SCALE_MAX, FONT_SCALE_MIN,
    RADIUS_MAX, RADIUS_MIN, SPACING_MAX, SPACING_MIN,
};
use crate::repository::Repository;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::validation::ValidatedJson;

/// `GET /api/v1/users/{user_id}/profile`
pub async fn get_profile(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<ProfileResponse>>, AppError> {
    ensure_scope(&auth_user, &user_id)?;

    let repository = Repository::from_db(&state.db);
    let profile = load_or_cold_start(&repository, &user_id).await?;

    Ok(Json(ApiResponse::success(profile.to_response())))
}

/// `PUT /api/v1/users/{user_id}/profile`
pub async fn put_profile(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    auth_user: AuthUser,
    ValidatedJson(request): ValidatedJson<UpdateProfileRequest>,
) -> Result<Json<ApiResponse<ProfileResponse>>, AppError> {
    ensure_scope(&auth_user, &user_id)?;
    let update = request.validate()?;

    let repository = Repository::from_db(&state.db);
    let profile = load_or_cold_start(&repository, &user_id).await?;
    let updated = build_engine(&state).apply_explicit(&profile, &update, Utc::now());
    repository.upsert_user_profile(&updated).await?;

    Ok(Json(ApiResponse::success(updated.to_response())))
}

/// `POST /api/v1/users/{user_id}/profile/refresh`
pub async fn refresh_profile(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    auth_user: AuthUser,
    ValidatedJson(request): ValidatedJson<RefreshProfileRequest>,
) -> Result<Json<ApiResponse<RefreshProfileResponse>>, AppError> {
    ensure_scope(&auth_user, &user_id)?;
    request.validate(state.config.profile_max_observations)?;

    let repository = Repository::from_db(&state.db);
    let profile = load_or_cold_start(&repository, &user_id).await?;
    let now = Utc::now();
    let observations = collect_observations(&state, &repository, &user_id, &request, now).await?;

    let outcome =
        build_engine(&state).apply_observations(&profile, &observations, now, request.force);
    if outcome.updated {
        repository.upsert_user_profile(&outcome.profile).await?;
    }

    Ok(Json(ApiResponse::success(RefreshProfileResponse {
        updated: outcome.updated,
        observed_samples: outcome.observed_samples,
        skipped_reason: outcome.skipped_reason,
        profile: outcome.profile.to_response(),
    })))
}

/// 画像刷新响应。
#[derive(Debug, Serialize)]
pub struct RefreshProfileResponse {
    /// 是否实际更新并落库。
    pub updated: bool,
    /// 参与计算的样本数。
    pub observed_samples: usize,
    /// 未更新原因。
    pub skipped_reason: Option<UpdateSkipReason>,
    /// 当前画像。
    pub profile: ProfileResponse,
}

/// 显式画像写入请求。
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateProfileRequest {
    /// 字号缩放。
    pub font_scale: Option<f64>,
    /// 密度。
    pub density: Option<f64>,
    /// 明暗主题。
    pub theme: Option<Theme>,
    /// 圆角（px）。
    pub radius: Option<f64>,
    /// 间距缩放。
    pub spacing: Option<f64>,
}

impl UpdateProfileRequest {
    fn validate(&self) -> Result<ProfileUpdate, AppError> {
        let update = ProfileUpdate {
            font_scale: self.font_scale,
            density: self.density,
            theme: self.theme,
            radius: self.radius,
            spacing: self.spacing,
        };

        if update.is_empty() {
            return Err(AppError::invalid_param(
                "至少需要提供 font_scale / density / theme / radius / spacing 中的一个字段",
            ));
        }

        if let Some(value) = update.font_scale {
            validate_range("font_scale", value, FONT_SCALE_MIN, FONT_SCALE_MAX)?;
        }
        if let Some(value) = update.density {
            validate_range("density", value, DENSITY_MIN, DENSITY_MAX)?;
        }
        if let Some(value) = update.radius {
            validate_range("radius", value, RADIUS_MIN, RADIUS_MAX)?;
        }
        if let Some(value) = update.spacing {
            validate_range("spacing", value, SPACING_MIN, SPACING_MAX)?;
        }

        Ok(update)
    }
}

/// 画像刷新请求。
#[derive(Debug, Deserialize, Validate)]
pub struct RefreshProfileRequest {
    /// 跳过更新频率控制（运维 / 离线补算）。
    #[serde(default)]
    pub force: bool,
    /// 回看窗口（小时），仅在不显式传样本时生效。
    #[serde(default = "default_window_hours")]
    pub window_hours: i64,
    /// 显式传入的埋点样本。
    #[serde(default)]
    pub telemetry: Vec<TelemetryInput>,
    /// 显式传入的调研样本。
    #[serde(default)]
    pub surveys: Vec<SurveyInput>,
}

impl RefreshProfileRequest {
    fn validate(&self, max_observations: usize) -> Result<(), AppError> {
        if !(1..=720).contains(&self.window_hours) {
            return Err(AppError::invalid_param("window_hours 必须在 1..=720 之间"));
        }

        let supplied = self.telemetry.len().saturating_add(self.surveys.len());
        if supplied > max_observations {
            return Err(AppError::invalid_param(format!(
                "单次最多提交 {max_observations} 条样本"
            )));
        }

        for input in &self.telemetry {
            input.validate()?;
        }
        for input in &self.surveys {
            input.validate()?;
        }

        Ok(())
    }
}

/// 显式埋点样本。
#[derive(Debug, Deserialize)]
pub struct TelemetryInput {
    /// 停留时长（毫秒）。
    pub dwell_time: i64,
    /// 元素点击率。
    #[serde(default)]
    pub element_click_rate: f64,
    /// 误触率。
    #[serde(default)]
    pub misclick_rate: f64,
    /// 滚动深度。
    #[serde(default)]
    pub scroll_depth: f64,
    /// 发生时间；缺省为当前时间。
    pub occurred_at: Option<DateTime<Utc>>,
}

impl TelemetryInput {
    fn validate(&self) -> Result<(), AppError> {
        if !(0..=86_400_000).contains(&self.dwell_time) {
            return Err(AppError::invalid_param(
                "dwell_time 必须在 0..=86400000 毫秒之间",
            ));
        }
        validate_rate("element_click_rate", self.element_click_rate)?;
        validate_rate("misclick_rate", self.misclick_rate)?;
        validate_rate("scroll_depth", self.scroll_depth)?;
        Ok(())
    }
}

/// 显式调研样本。
#[derive(Debug, Deserialize)]
pub struct SurveyInput {
    /// 选中的风格 ID。
    pub selected_style_id: String,
    /// 拒绝的风格 ID。
    pub rejected_style_id: String,
    /// 发生时间；缺省为当前时间。
    pub occurred_at: Option<DateTime<Utc>>,
}

impl SurveyInput {
    fn validate(&self) -> Result<(), AppError> {
        validate_style_id("selected_style_id", &self.selected_style_id)?;
        validate_style_id("rejected_style_id", &self.rejected_style_id)?;
        if self.selected_style_id == self.rejected_style_id {
            return Err(AppError::invalid_param(
                "selected_style_id 与 rejected_style_id 不能相同",
            ));
        }
        Ok(())
    }
}

async fn load_or_cold_start(
    repository: &Repository,
    user_id: &str,
) -> Result<UserProfile, AppError> {
    match repository.find_user_profile(user_id).await? {
        Some(profile) => Ok(profile),
        None => Ok(UserProfile::cold_start(user_id.to_string(), Utc::now())),
    }
}

async fn collect_observations(
    state: &AppState,
    repository: &Repository,
    user_id: &str,
    request: &RefreshProfileRequest,
    now: DateTime<Utc>,
) -> Result<Vec<Observation>, AppError> {
    let mut observations: Vec<Observation> = request
        .telemetry
        .iter()
        .map(|input| {
            Observation::from_telemetry(
                input.dwell_time,
                input.element_click_rate,
                input.misclick_rate,
                input.scroll_depth,
                input.occurred_at.unwrap_or(now),
            )
        })
        .collect();

    observations.extend(request.surveys.iter().map(|input| {
        Observation::from_survey(
            &input.selected_style_id,
            &input.rejected_style_id,
            input.occurred_at.unwrap_or(now),
        )
    }));

    if !observations.is_empty() {
        return Ok(observations);
    }

    let limit = state.config.profile_max_observations as i64;
    let since = now - Duration::hours(request.window_hours);
    let telemetry = repository
        .list_profile_telemetry_samples(user_id, since, limit)
        .await?;
    let surveys = repository
        .list_profile_survey_samples(user_id, since, limit)
        .await?;

    observations.extend(
        telemetry
            .into_iter()
            .map(|sample| sample.into_observation()),
    );
    observations.extend(surveys.into_iter().map(|sample| sample.into_observation()));
    observations.truncate(state.config.profile_max_observations);

    Ok(observations)
}

fn build_engine(state: &AppState) -> ProfileEngine {
    ProfileEngine::new(ProfileEngineConfig {
        base_alpha: state.config.profile_ewma_alpha,
        max_alpha: state.config.profile_max_alpha,
        min_update_interval: Duration::seconds(
            state.config.profile_min_update_interval_secs as i64,
        ),
    })
}

fn ensure_scope(auth_user: &AuthUser, user_id: &str) -> Result<(), AppError> {
    if auth_user.user_id != user_id {
        return Err(AppError::forbidden());
    }
    if user_id.is_empty() || user_id.len() > 64 {
        return Err(AppError::invalid_param("user_id 长度必须在 1..=64 之间"));
    }
    Ok(())
}

fn validate_range(field: &str, value: f64, min: f64, max: f64) -> Result<(), AppError> {
    if !value.is_finite() || value < min || value > max {
        return Err(AppError::invalid_param(format!(
            "{field} 必须在 {min}..={max} 之间"
        )));
    }
    Ok(())
}

fn validate_rate(field: &str, value: f64) -> Result<(), AppError> {
    validate_range(field, value, 0.0, 1.0)
}

fn validate_style_id(field: &str, value: &str) -> Result<(), AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(AppError::invalid_param(format!(
            "{field} 长度必须在 1..=128 之间"
        )));
    }
    Ok(())
}

fn default_window_hours() -> i64 {
    24
}
