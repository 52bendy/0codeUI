//! 用户画像领域模型（REQ-202610-1176）。
//!
//! 偏好向量采用 PRD §4.1 的 `{font_scale, density, theme, radius, spacing}` 结构：
//! - `font_scale`：字号缩放，1.0 为默认；
//! - `density`：卡片 / 列表密度，0.0 紧凑、1.0 宽松；
//! - `theme`：明暗主题，由内部连续值 `theme_score` 以 0.5 为阈值映射；
//! - `radius`：按钮 / 卡片圆角（px）；
//! - `spacing`：间距缩放，1.0 为默认。
//!
//! 对外响应同时提供 `preferences`（完整画像）与 `prompt_profile`
//! （可直接序列化注入 14 Prompt 拼装器的精简 JSON）。

use chrono::{DateTime, Utc};
use serde::Serialize;

/// 字号缩放下限。
pub const FONT_SCALE_MIN: f64 = 0.8;
/// 字号缩放上限。
pub const FONT_SCALE_MAX: f64 = 1.4;
/// 密度下限（紧凑）。
pub const DENSITY_MIN: f64 = 0.0;
/// 密度上限（宽松）。
pub const DENSITY_MAX: f64 = 1.0;
/// 圆角下限（px）。
pub const RADIUS_MIN: f64 = 0.0;
/// 圆角上限（px）。
pub const RADIUS_MAX: f64 = 24.0;
/// 间距缩放下限。
pub const SPACING_MIN: f64 = 0.75;
/// 间距缩放上限。
pub const SPACING_MAX: f64 = 1.5;
/// 暗色主题判定阈值。
pub const THEME_DARK_THRESHOLD: f64 = 0.5;

/// 明暗主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// 明亮主题。
    Light,
    /// 暗黑主题。
    Dark,
}

impl Theme {
    /// 连续主题分数 → 离散主题。
    pub fn from_score(score: f64) -> Self {
        if score >= THEME_DARK_THRESHOLD {
            Self::Dark
        } else {
            Self::Light
        }
    }

    /// 离散主题 → 连续主题分数。
    pub fn score(self) -> f64 {
        match self {
            Self::Light => 0.0,
            Self::Dark => 1.0,
        }
    }
}

/// 画像置信度等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    /// 冷启动：尚无任何行为 / 调研样本。
    ColdStart,
    /// 低置信度（< 0.3）。
    Low,
    /// 中置信度（0.3 ~ 0.7）。
    Medium,
    /// 高置信度（>= 0.7）。
    High,
}

impl ConfidenceLevel {
    /// 按样本量 / 置信度划分等级。
    pub fn from_profile(sample_count: u64, confidence: f64) -> Self {
        if sample_count == 0 {
            Self::ColdStart
        } else if confidence < 0.3 {
            Self::Low
        } else if confidence < 0.7 {
            Self::Medium
        } else {
            Self::High
        }
    }
}

/// 连续偏好向量（持久化与 EWMA 计算使用）。
#[derive(Debug, Clone, PartialEq)]
pub struct PreferenceVector {
    /// 字号缩放。
    pub font_scale: f64,
    /// 密度。
    pub density: f64,
    /// 主题连续分数（0 = 明亮，1 = 暗黑）。
    pub theme_score: f64,
    /// 圆角（px）。
    pub radius: f64,
    /// 间距缩放。
    pub spacing: f64,
}

impl Default for PreferenceVector {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            density: 0.5,
            theme_score: 0.0,
            radius: 8.0,
            spacing: 1.0,
        }
    }
}

impl PreferenceVector {
    /// 当前离散主题。
    pub fn theme(&self) -> Theme {
        Theme::from_score(self.theme_score)
    }

    /// 把各维度收敛到合法区间，避免异常输入写坏画像。
    pub fn sanitized(self) -> Self {
        Self {
            font_scale: self.font_scale.clamp(FONT_SCALE_MIN, FONT_SCALE_MAX),
            density: self.density.clamp(DENSITY_MIN, DENSITY_MAX),
            theme_score: self.theme_score.clamp(0.0, 1.0),
            radius: self.radius.clamp(RADIUS_MIN, RADIUS_MAX),
            spacing: self.spacing.clamp(SPACING_MIN, SPACING_MAX),
        }
    }

    /// 对外展示结构（主题为离散值）。
    pub fn public(&self) -> PublicPreferences {
        PublicPreferences {
            font_scale: self.font_scale,
            density: self.density,
            theme: self.theme(),
            radius: self.radius,
            spacing: self.spacing,
        }
    }
}

/// 对外展示的偏好向量。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PublicPreferences {
    /// 字号缩放。
    pub font_scale: f64,
    /// 密度。
    pub density: f64,
    /// 明暗主题。
    pub theme: Theme,
    /// 圆角（px）。
    pub radius: f64,
    /// 间距缩放。
    pub spacing: f64,
}

/// 用户画像（持久化领域对象）。
#[derive(Debug, Clone, PartialEq)]
pub struct UserProfile {
    /// 用户标识。
    pub user_id: String,
    /// 偏好向量。
    pub preferences: PreferenceVector,
    /// 置信度（0.0 ~ 1.0）。
    pub confidence: f64,
    /// 累计参与计算的样本数。
    pub sample_count: u64,
    /// 画像版本（每次有效更新 +1）。
    pub version: u64,
    /// 是否为冷启动默认画像。
    pub cold_start: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 最近一次画像更新时间；从未更新时为 `None`。
    pub updated_at: Option<DateTime<Utc>>,
    /// 最近一次行为样本发生时间。
    pub last_observed_at: Option<DateTime<Utc>>,
}

impl UserProfile {
    /// 构造冷启动默认画像。
    pub fn cold_start(user_id: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            user_id: user_id.into(),
            preferences: PreferenceVector::default(),
            confidence: 0.0,
            sample_count: 0,
            version: 1,
            cold_start: true,
            created_at: now,
            updated_at: None,
            last_observed_at: None,
        }
    }

    /// 置信度等级。
    pub fn confidence_level(&self) -> ConfidenceLevel {
        ConfidenceLevel::from_profile(self.sample_count, self.confidence)
    }

    /// 可直接注入 Prompt 的精简画像 JSON。
    pub fn prompt_profile(&self) -> PromptProfile {
        PromptProfile {
            font_scale: self.preferences.font_scale,
            density: self.preferences.density,
            theme: self.preferences.theme(),
            radius: self.preferences.radius,
            spacing: self.preferences.spacing,
            confidence: self.confidence,
            cold_start: self.cold_start,
        }
    }

    /// 转为 HTTP 响应结构。
    pub fn to_response(&self) -> ProfileResponse {
        ProfileResponse {
            user_id: self.user_id.clone(),
            preferences: self.preferences.public(),
            confidence: self.confidence,
            confidence_level: self.confidence_level(),
            cold_start: self.cold_start,
            sample_count: self.sample_count,
            version: self.version,
            updated_at: self.updated_at,
            prompt_profile: self.prompt_profile(),
        }
    }
}

/// 可直接注入 Prompt 的精简画像。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PromptProfile {
    /// 字号缩放。
    pub font_scale: f64,
    /// 密度。
    pub density: f64,
    /// 明暗主题。
    pub theme: Theme,
    /// 圆角（px）。
    pub radius: f64,
    /// 间距缩放。
    pub spacing: f64,
    /// 置信度。
    pub confidence: f64,
    /// 是否冷启动。
    pub cold_start: bool,
}

/// 画像 HTTP 响应。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileResponse {
    /// 用户标识。
    pub user_id: String,
    /// 偏好向量。
    pub preferences: PublicPreferences,
    /// 置信度。
    pub confidence: f64,
    /// 置信度等级。
    pub confidence_level: ConfidenceLevel,
    /// 是否冷启动。
    pub cold_start: bool,
    /// 累计样本数。
    pub sample_count: u64,
    /// 画像版本。
    pub version: u64,
    /// 最近更新时间。
    pub updated_at: Option<DateTime<Utc>>,
    /// 可直接注入 Prompt 的精简画像。
    pub prompt_profile: PromptProfile,
}

/// 显式画像写入（用户主动设置，不经过 EWMA）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileUpdate {
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

impl ProfileUpdate {
    /// 是否未提供任何字段。
    pub fn is_empty(&self) -> bool {
        self.font_scale.is_none()
            && self.density.is_none()
            && self.theme.is_none()
            && self.radius.is_none()
            && self.spacing.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 22, 8, 0, 0).unwrap()
    }

    #[test]
    fn cold_start_profile_has_expected_defaults() {
        let profile = UserProfile::cold_start("u1", now());
        assert_eq!(profile.preferences.font_scale, 1.0);
        assert_eq!(profile.preferences.density, 0.5);
        assert_eq!(profile.preferences.theme(), Theme::Light);
        assert_eq!(profile.preferences.radius, 8.0);
        assert_eq!(profile.preferences.spacing, 1.0);
        assert_eq!(profile.confidence_level(), ConfidenceLevel::ColdStart);
        assert!(profile.cold_start);
    }

    #[test]
    fn sanitize_clamps_out_of_range_values() {
        let vector = PreferenceVector {
            font_scale: 9.0,
            density: -1.0,
            theme_score: 2.0,
            radius: 99.0,
            spacing: 0.1,
        }
        .sanitized();
        assert_eq!(vector.font_scale, FONT_SCALE_MAX);
        assert_eq!(vector.density, DENSITY_MIN);
        assert_eq!(vector.theme_score, 1.0);
        assert_eq!(vector.radius, RADIUS_MAX);
        assert_eq!(vector.spacing, SPACING_MIN);
    }

    #[test]
    fn theme_threshold_is_half() {
        assert_eq!(Theme::from_score(0.49), Theme::Light);
        assert_eq!(Theme::from_score(0.50), Theme::Dark);
        assert_eq!(Theme::Dark.score(), 1.0);
    }
}
