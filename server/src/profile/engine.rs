//! 用户画像引擎：EWMA 偏好计算与防抖动。
//!
//! 防抖动分三层：
//! 1. **单步限幅**：任一维度单次更新最多移动固定步长，异常停留 / 误触无法造成 UI 跳变；
//! 2. **冷启动死区**：首次采样若变化小于死区阈值，不离开冷启动默认画像；
//! 3. **更新频率控制**：两次画像更新之间至少间隔 `min_update_interval`，
//!    高频埋点不会反复触发前端重排；`force = true` 供离线补算 / 运维使用。
//!
//! EWMA 公式：`next = current + clamp((target - current) * alpha, -max_step, max_step)`。
//! `alpha` 随同一批观测数量小幅提高，但不超过 `max_alpha`。

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use super::model::{
    PreferenceVector, ProfileUpdate, UserProfile, DENSITY_MAX, DENSITY_MIN, FONT_SCALE_MAX,
    FONT_SCALE_MIN, RADIUS_MAX, RADIUS_MIN, SPACING_MAX, SPACING_MIN,
};
use super::signals::Observation;

/// 单步限幅（防止一次异常信号造成 UI 剧烈抖动）。
const MAX_STEP_FONT_SCALE: f64 = 0.05;
const MAX_STEP_DENSITY: f64 = 0.08;
const MAX_STEP_THEME: f64 = 0.10;
const MAX_STEP_RADIUS: f64 = 2.0;
const MAX_STEP_SPACING: f64 = 0.06;

/// 冷启动死区：低于该变化量的首次采样视为测量噪声。
const DEADBAND_FONT_SCALE: f64 = 0.002;
const DEADBAND_DENSITY: f64 = 0.003;
const DEADBAND_THEME: f64 = 0.004;
const DEADBAND_RADIUS: f64 = 0.05;
const DEADBAND_SPACING: f64 = 0.002;

/// 置信度饱和样本数：`confidence = 1 - exp(-sample_count / 20)`。
const CONFIDENCE_SATURATION: f64 = 20.0;

/// 显式写入折算的等效样本数（用户主动设置可信度高）。
const EXPLICIT_EQUIVALENT_SAMPLES: u64 = 20;

/// 画像引擎配置。
#[derive(Debug, Clone)]
pub struct ProfileEngineConfig {
    /// 基础 EWMA 平滑系数。
    pub base_alpha: f64,
    /// EWMA 平滑系数上限。
    pub max_alpha: f64,
    /// 两次自动更新之间的最小间隔。
    pub min_update_interval: Duration,
}

impl Default for ProfileEngineConfig {
    fn default() -> Self {
        Self {
            base_alpha: 0.15,
            max_alpha: 0.6,
            min_update_interval: Duration::seconds(60),
        }
    }
}

/// 跳过更新的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSkipReason {
    /// 距上次更新不足最小间隔。
    RateLimited,
    /// 变化量低于冷启动死区。
    NoChange,
    /// 没有任何可用观测。
    NoObservations,
}

/// 一次画像更新结果。
#[derive(Debug, Clone)]
pub struct UpdateOutcome {
    /// 更新后的画像（未更新时为原画像副本）。
    pub profile: UserProfile,
    /// 是否实际写入。
    pub updated: bool,
    /// 未写入时的原因。
    pub skipped_reason: Option<UpdateSkipReason>,
    /// 参与本次计算的观测条数。
    pub observed_samples: usize,
}

impl UpdateOutcome {
    fn skipped(profile: &UserProfile, reason: UpdateSkipReason, observed_samples: usize) -> Self {
        Self {
            profile: profile.clone(),
            updated: false,
            skipped_reason: Some(reason),
            observed_samples,
        }
    }
}

/// 用户画像引擎（无状态，可安全复用）。
#[derive(Debug, Clone)]
pub struct ProfileEngine {
    config: ProfileEngineConfig,
}

impl ProfileEngine {
    /// 构造引擎。
    pub fn new(config: ProfileEngineConfig) -> Self {
        Self { config }
    }

    /// 当前配置。
    pub fn config(&self) -> &ProfileEngineConfig {
        &self.config
    }

    /// 构造冷启动默认画像。
    pub fn cold_start(&self, user_id: impl Into<String>, now: DateTime<Utc>) -> UserProfile {
        UserProfile::cold_start(user_id, now)
    }

    /// 用一批行为观测更新画像。
    pub fn apply_observations(
        &self,
        profile: &UserProfile,
        observations: &[Observation],
        now: DateTime<Utc>,
        force: bool,
    ) -> UpdateOutcome {
        if observations.is_empty() {
            return UpdateOutcome::skipped(profile, UpdateSkipReason::NoObservations, 0);
        }

        if !force {
            if let Some(updated_at) = profile.updated_at {
                if now.signed_duration_since(updated_at) < self.config.min_update_interval {
                    return UpdateOutcome::skipped(
                        profile,
                        UpdateSkipReason::RateLimited,
                        observations.len(),
                    );
                }
            }
        }

        let mut accumulators = [Accumulator::default(); 5];
        for observation in observations {
            let weight = (observation.weight * observation.source.weight_factor()).max(0.0);
            if weight == 0.0 {
                continue;
            }
            accumulators[0].add(observation.vector.font_scale, weight);
            accumulators[1].add(observation.vector.density, weight);
            accumulators[2].add(observation.vector.theme_score, weight);
            accumulators[3].add(observation.vector.radius, weight);
            accumulators[4].add(observation.vector.spacing, weight);
        }

        if accumulators
            .iter()
            .all(|accumulator| accumulator.weight == 0.0)
        {
            return UpdateOutcome::skipped(profile, UpdateSkipReason::NoObservations, 0);
        }

        let is_cold_start = profile.sample_count == 0;
        let next = PreferenceVector {
            font_scale: self
                .step(
                    profile.preferences.font_scale,
                    accumulators[0],
                    MAX_STEP_FONT_SCALE,
                    DEADBAND_FONT_SCALE,
                    is_cold_start,
                )
                .clamp(FONT_SCALE_MIN, FONT_SCALE_MAX),
            density: self
                .step(
                    profile.preferences.density,
                    accumulators[1],
                    MAX_STEP_DENSITY,
                    DEADBAND_DENSITY,
                    is_cold_start,
                )
                .clamp(DENSITY_MIN, DENSITY_MAX),
            theme_score: self
                .step(
                    profile.preferences.theme_score,
                    accumulators[2],
                    MAX_STEP_THEME,
                    DEADBAND_THEME,
                    is_cold_start,
                )
                .clamp(0.0, 1.0),
            radius: self
                .step(
                    profile.preferences.radius,
                    accumulators[3],
                    MAX_STEP_RADIUS,
                    DEADBAND_RADIUS,
                    is_cold_start,
                )
                .clamp(RADIUS_MIN, RADIUS_MAX),
            spacing: self
                .step(
                    profile.preferences.spacing,
                    accumulators[4],
                    MAX_STEP_SPACING,
                    DEADBAND_SPACING,
                    is_cold_start,
                )
                .clamp(SPACING_MIN, SPACING_MAX),
        };

        if next == profile.preferences {
            return UpdateOutcome::skipped(profile, UpdateSkipReason::NoChange, observations.len());
        }

        let sample_count = profile
            .sample_count
            .saturating_add(observations.len() as u64);
        let last_observed_at = observations
            .iter()
            .map(|observation| observation.observed_at)
            .max()
            .or(profile.last_observed_at);

        UpdateOutcome {
            profile: UserProfile {
                user_id: profile.user_id.clone(),
                preferences: next.sanitized(),
                confidence: confidence_for(sample_count),
                sample_count,
                version: profile.version.saturating_add(1),
                cold_start: false,
                created_at: profile.created_at,
                updated_at: Some(now),
                last_observed_at,
            },
            updated: true,
            skipped_reason: None,
            observed_samples: observations.len(),
        }
    }

    /// 应用用户主动设置（不经过 EWMA，直接覆盖指定维度）。
    pub fn apply_explicit(
        &self,
        profile: &UserProfile,
        update: &ProfileUpdate,
        now: DateTime<Utc>,
    ) -> UserProfile {
        if update.is_empty() {
            return profile.clone();
        }

        let next = PreferenceVector {
            font_scale: update.font_scale.unwrap_or(profile.preferences.font_scale),
            density: update.density.unwrap_or(profile.preferences.density),
            theme_score: update
                .theme
                .map(|theme| theme.score())
                .unwrap_or(profile.preferences.theme_score),
            radius: update.radius.unwrap_or(profile.preferences.radius),
            spacing: update.spacing.unwrap_or(profile.preferences.spacing),
        }
        .sanitized();

        let sample_count = profile
            .sample_count
            .saturating_add(EXPLICIT_EQUIVALENT_SAMPLES);

        UserProfile {
            user_id: profile.user_id.clone(),
            preferences: next,
            confidence: confidence_for(sample_count),
            sample_count,
            version: profile.version.saturating_add(1),
            cold_start: false,
            created_at: profile.created_at,
            updated_at: Some(now),
            last_observed_at: profile.last_observed_at,
        }
    }

    fn step(
        &self,
        current: f64,
        accumulator: Accumulator,
        max_step: f64,
        deadband: f64,
        is_cold_start: bool,
    ) -> f64 {
        let Some(target) = accumulator.target() else {
            return current;
        };

        let alpha = (self.config.base_alpha
            * (1.0 + 0.1 * accumulator.count.saturating_sub(1) as f64))
            .clamp(0.0, self.config.max_alpha);
        let delta = (target - current) * alpha;
        let clamped = delta.clamp(-max_step, max_step);

        if is_cold_start && clamped.abs() < deadband {
            return current;
        }

        current + clamped
    }
}

/// 置信度：随累计样本数饱和到 1.0。
pub fn confidence_for(sample_count: u64) -> f64 {
    (1.0 - (-(sample_count as f64) / CONFIDENCE_SATURATION).exp()).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, Default)]
struct Accumulator {
    weighted_sum: f64,
    weight: f64,
    count: u64,
}

impl Accumulator {
    fn add(&mut self, value: Option<f64>, weight: f64) {
        if let Some(value) = value {
            if value.is_finite() && weight > 0.0 {
                self.weighted_sum += value * weight;
                self.weight += weight;
                self.count = self.count.saturating_add(1);
            }
        }
    }

    fn target(&self) -> Option<f64> {
        if self.weight > 0.0 {
            Some(self.weighted_sum / self.weight)
        } else {
            None
        }
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
    fn confidence_grows_with_samples_and_saturates() {
        assert_eq!(confidence_for(0), 0.0);
        assert!((confidence_for(20) - 0.6321).abs() < 0.001);
        assert!(confidence_for(200) > 0.999);
    }

    #[test]
    fn empty_update_keeps_profile_unchanged() {
        let engine = ProfileEngine::new(ProfileEngineConfig::default());
        let profile = engine.cold_start("u1", now());
        let updated = engine.apply_explicit(&profile, &ProfileUpdate::default(), now());
        assert_eq!(updated, profile);
    }
}
