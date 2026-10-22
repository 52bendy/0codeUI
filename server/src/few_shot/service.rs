//! Few-Shot 检索编排。
//!
//! 检索顺序：
//! 1. 功能开关关闭 → `disabled` 安全降级；
//! 2. 加载画像：08 持久化画像优先，其次用 06 埋点 / 07 调研即时推导，最后冷启动；
//! 3. 按场景取候选片段；空库 → `empty_library`；
//! 4. 语义 + 画像 + 质量加权排序；全部低于阈值 → `low_confidence`；
//! 5. 按 token 预算渲染片段段落；一个都放不下 → `token_budget_exceeded`。
//!
//! 任何降级都不会返回错误，只会返回空的 `prompt_section`，保证生成主链路可用。

use chrono::{Duration, Utc};

use crate::error::{AppError, ErrorCode};
use crate::profile::{merge_targets, Observation, PreferenceVector};
use crate::repository::{new_id, Repository, RepositoryError};

use super::model::{
    derive_snippet_text, ComponentSnippet, DegradeReason, FewShotConfig, FewShotQuery,
    FewShotResult, ProfileSource, RegisterSnippetRequest,
};
use super::prompt::{append_to_prompt, render_prompt_section};
use super::rank::rank_candidates;
use super::text::tokenize;

/// 信号推导画像的回看窗口（天）。
const SIGNAL_WINDOW_DAYS: i64 = 30;
/// 单次信号推导最多读取的样本数。
const SIGNAL_SAMPLE_LIMIT: i64 = 200;

/// Few-Shot 服务错误。
#[derive(Debug, thiserror::Error)]
pub enum FewShotError {
    /// 入参非法。
    #[error("{0}")]
    Invalid(String),
    /// 片段不存在。
    #[error("snippet not found: {0}")]
    NotFound(String),
    /// 仓储错误。
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl From<FewShotError> for AppError {
    fn from(error: FewShotError) -> Self {
        match error {
            FewShotError::Invalid(message) => AppError::invalid_param(message),
            FewShotError::NotFound(message) => {
                AppError::new(ErrorCode::NotFound, format!("snippet not found: {message}"))
            }
            FewShotError::Repository(error) => error.into(),
        }
    }
}

/// Few-Shot 检索服务。
#[derive(Debug, Clone)]
pub struct FewShotService {
    repository: Repository,
    config: FewShotConfig,
}

impl FewShotService {
    /// 构造服务。
    pub fn new(repository: Repository, config: FewShotConfig) -> Self {
        Self { repository, config }
    }

    /// 当前配置（路由层用于写接口令牌校验）。
    pub fn config(&self) -> &FewShotConfig {
        &self.config
    }

    /// 检索并渲染 Prompt 片段。
    pub async fn retrieve(
        &self,
        user_id: &str,
        query: FewShotQuery,
    ) -> Result<FewShotResult, FewShotError> {
        if !self.config.enabled {
            return Ok(self.degraded(&query, DegradeReason::Disabled, 0, ProfileSource::ColdStart));
        }

        let signals = self.load_signals(user_id, &query.scene_code).await?;
        let candidates = self
            .repository
            .list_few_shot_candidates(&query.scene_code, self.config.candidate_limit as i64)
            .await?;
        let candidate_count = candidates.len();
        if candidates.is_empty() {
            return Ok(self.degraded(
                &query,
                DegradeReason::EmptyLibrary,
                candidate_count,
                signals.source,
            ));
        }

        let query_tokens = query.query.as_deref().map(tokenize).unwrap_or_default();
        let ranked = rank_candidates(
            &candidates,
            &query_tokens,
            signals.profile.as_ref(),
            signals.engagement,
        );
        let above_threshold: Vec<_> = ranked
            .into_iter()
            .filter(|scored| scored.score >= self.config.min_score)
            .collect();
        if above_threshold.is_empty() {
            return Ok(self.degraded(
                &query,
                DegradeReason::LowConfidence,
                candidate_count,
                signals.source,
            ));
        }

        let top_k = query.top_k.unwrap_or(self.config.top_k).clamp(1, 10);
        let token_budget = query
            .token_budget
            .unwrap_or(self.config.token_budget)
            .max(64);
        let render = render_prompt_section(
            &above_threshold,
            top_k,
            self.config.max_snippet_chars,
            token_budget,
        );
        if render.snippets.is_empty() {
            return Ok(self.degraded(
                &query,
                DegradeReason::TokenBudgetExceeded,
                candidate_count,
                signals.source,
            ));
        }

        let final_prompt = append_to_prompt(query.base_prompt.as_deref(), &render.section);
        Ok(FewShotResult {
            scene_code: query.scene_code,
            degraded: false,
            degraded_reason: None,
            candidate_count,
            snippets: render.snippets,
            prompt_section: render.section,
            final_prompt,
            token_estimate: render.token_estimate,
            profile_source: signals.source,
        })
    }

    /// 登记片段；`source_ref` 相同则幂等更新并保留反馈统计。
    pub async fn register(
        &self,
        request: RegisterSnippetRequest,
    ) -> Result<ComponentSnippet, FewShotError> {
        request.validate().map_err(FewShotError::Invalid)?;

        let existing = match (&request.source_ref, request.source_type) {
            (Some(source_ref), source_type) => {
                self.repository
                    .find_few_shot_snippet_by_source(source_type, source_ref)
                    .await?
            }
            _ => None,
        };

        let now = Utc::now();
        let id = request
            .id
            .clone()
            .or_else(|| existing.as_ref().map(|snippet| snippet.id.clone()))
            .unwrap_or_else(|| new_id("FSP"));
        let created_at = existing
            .as_ref()
            .map(|snippet| snippet.created_at)
            .unwrap_or(now);
        let usage_count = existing.as_ref().map_or(0, |snippet| snippet.usage_count);
        let positive_feedback = existing
            .as_ref()
            .map_or(0, |snippet| snippet.positive_feedback);
        let negative_feedback = existing
            .as_ref()
            .map_or(0, |snippet| snippet.negative_feedback);
        let satisfaction = request
            .satisfaction
            .or_else(|| existing.as_ref().map(|snippet| snippet.satisfaction))
            .unwrap_or(0.5);
        let implicit_score = request
            .implicit_score
            .or_else(|| existing.as_ref().map(|snippet| snippet.implicit_score))
            .unwrap_or(0.5);

        let snippet = ComponentSnippet {
            id,
            source_type: request.source_type,
            source_ref: request.source_ref,
            scene_code: request.scene_code,
            title: request.title,
            snippet_text: request
                .snippet_text
                .unwrap_or_else(|| derive_snippet_text(&request.snippet_html)),
            snippet_html: request.snippet_html,
            tags: request.tags,
            style: request.style,
            satisfaction,
            implicit_score,
            usage_count,
            positive_feedback,
            negative_feedback,
            enabled: true,
            created_at,
            updated_at: now,
        };
        self.repository.upsert_few_shot_snippet(&snippet).await?;
        Ok(snippet)
    }

    /// 记录片段反馈。
    pub async fn feedback(
        &self,
        id: &str,
        satisfaction: f64,
    ) -> Result<ComponentSnippet, FewShotError> {
        if !(0.0..=1.0).contains(&satisfaction) || !satisfaction.is_finite() {
            return Err(FewShotError::Invalid(
                "satisfaction 必须在 0.0..=1.0 之间".to_string(),
            ));
        }
        self.repository
            .apply_few_shot_feedback(id, satisfaction, Utc::now())
            .await?
            .ok_or_else(|| FewShotError::NotFound(id.to_string()))
    }

    /// 加载用户画像 / 场景信号。
    async fn load_signals(
        &self,
        user_id: &str,
        scene_code: &str,
    ) -> Result<UserSignals, FewShotError> {
        let now = Utc::now();
        let since = now - Duration::days(SIGNAL_WINDOW_DAYS);
        let telemetry = self
            .repository
            .list_scene_telemetry_samples(user_id, scene_code, since, SIGNAL_SAMPLE_LIMIT)
            .await?;
        let engagement = average_engagement(&telemetry);

        if let Some(profile) = self.repository.find_user_profile(user_id).await? {
            if !profile.cold_start && profile.sample_count > 0 {
                return Ok(UserSignals {
                    profile: Some(profile.preferences),
                    engagement,
                    source: ProfileSource::Profile,
                });
            }
        }

        let surveys = self
            .repository
            .list_scene_survey_samples(user_id, scene_code, since, SIGNAL_SAMPLE_LIMIT)
            .await?;
        let mut observations: Vec<Observation> = telemetry
            .into_iter()
            .map(|sample| sample.into_observation())
            .collect();
        observations.extend(surveys.into_iter().map(|sample| sample.into_observation()));

        if observations.is_empty() {
            return Ok(UserSignals {
                profile: None,
                engagement,
                source: ProfileSource::ColdStart,
            });
        }

        observations.sort_by_key(|observation| observation.observed_at);
        let profile = observations
            .iter()
            .fold(PreferenceVector::default(), |current, observation| {
                merge_targets(&current, observation)
            })
            .sanitized();

        Ok(UserSignals {
            profile: Some(profile),
            engagement,
            source: ProfileSource::Signals,
        })
    }

    fn degraded(
        &self,
        query: &FewShotQuery,
        reason: DegradeReason,
        candidate_count: usize,
        profile_source: ProfileSource,
    ) -> FewShotResult {
        FewShotResult {
            scene_code: query.scene_code.clone(),
            degraded: true,
            degraded_reason: Some(reason),
            candidate_count,
            snippets: Vec::new(),
            prompt_section: String::new(),
            final_prompt: append_to_prompt(query.base_prompt.as_deref(), ""),
            token_estimate: 0,
            profile_source,
        }
    }
}

/// 用户信号汇总。
struct UserSignals {
    profile: Option<PreferenceVector>,
    engagement: Option<f64>,
    source: ProfileSource,
}

/// 由埋点样本估算场景参与度（0.0 ~ 1.0）。
fn average_engagement(samples: &[crate::profile::TelemetrySample]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let total: f64 = samples
        .iter()
        .map(|sample| {
            let click = sanitize_rate(sample.element_click_rate);
            let misclick = sanitize_rate(sample.misclick_rate);
            let scroll = sanitize_rate(sample.scroll_depth);
            ((click - misclick).clamp(0.0, 1.0) + scroll) / 2.0
        })
        .sum();
    Some((total / samples.len() as f64).clamp(0.0, 1.0))
}

fn sanitize_rate(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_engagement_is_neutral_for_empty_samples() {
        assert_eq!(average_engagement(&[]), None);
    }

    #[test]
    fn average_engagement_clamps_abnormal_values() {
        let samples = vec![crate::profile::TelemetrySample {
            dwell_time_ms: 10_000,
            element_click_rate: 9.0,
            misclick_rate: -1.0,
            scroll_depth: 0.5,
            occurred_at: Utc::now(),
        }];
        let value = average_engagement(&samples).unwrap();
        assert!((0.0..=1.0).contains(&value));
    }
}
