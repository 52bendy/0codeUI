//! 生成管线编排：画像 / 业务数据 → Prompt → 模型 → 校验 → 混淆 → 返回。
//!
//! 关键约束：
//! - 端到端时间预算由 [`PipelineConfig::timeout_ms`] 控制，默认 1500ms；
//! - 进入动态阶段前先渲染一份静态模板兜底页，任何失败都能立即切换，保证不白屏；
//! - 模型超时、模型错误、输出校验失败、混淆失败都会降级并记录原因；
//! - 成功结果按内容指纹缓存，缓存命中仍重新混淆，避免固定指纹。

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::db::Db;
use crate::html_validator::validate_single_file_html;
use crate::model::{ModelError, ModelGateway, UiGeneration, UiRequest, Usage};
use crate::profile::{PromptProfile, UserProfile};
use crate::prompt::model::SceneConstraints;
use crate::prompt::{
    builtin_scene_constraints, PromptAssembler, PromptError, PROMPT_TEMPLATE_VERSION,
};
use crate::repository::Repository;
use crate::ui::scene::{SceneDefinition, REGISTRY};
use crate::ui_template::{DeviceInfo, RenderedPage, TemplateEngine, TemplateError};

use super::cache::{CachedGeneration, ResponseCache};
use super::config::PipelineConfig;
use super::metrics::PipelineMetrics;
use super::obfuscation::{HtmlObfuscator, NoopObfuscator};

/// 动态生成请求。
#[derive(Debug, Clone)]
pub struct PipelineRequest {
    /// 调用方用户标识（用于读取画像与鉴权归属校验）。
    pub user_id: String,
    /// 场景编码。
    pub scene_code: String,
    /// 设备信息。
    pub device: DeviceInfo,
    /// 业务数据。
    pub business_data: Value,
    /// 可选的用户意图描述；缺省使用场景展示名称。
    pub prompt: Option<String>,
    /// 显式指定模型（必须是网关已注册模型）。
    pub model: Option<String>,
    /// 会话标识（用于混淆类名映射）。
    pub session_id: Option<String>,
    /// 跳过缓存（调试 / 强制刷新）。
    pub bypass_cache: bool,
}

/// 降级原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradationReason {
    /// 管线总开关关闭。
    PipelineDisabled,
    /// 模型推理超时。
    ModelTimeout,
    /// 模型排队 / 并发繁忙。
    ModelBusy,
    /// 模型服务未启用。
    ModelDisabled,
    /// 其他模型错误。
    ModelError,
    /// 动态阶段耗尽时间预算。
    DeadlineExceeded,
    /// 输出未通过单文件 HTML 校验。
    ValidationFailed,
    /// 输出混淆失败。
    ObfuscationFailed,
}

impl DegradationReason {
    /// 稳定标识（用于指标与日志）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PipelineDisabled => "pipeline_disabled",
            Self::ModelTimeout => "model_timeout",
            Self::ModelBusy => "model_busy",
            Self::ModelDisabled => "model_disabled",
            Self::ModelError => "model_error",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::ValidationFailed => "validation_failed",
            Self::ObfuscationFailed => "obfuscation_failed",
        }
    }
}

/// 分阶段耗时（毫秒）。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PipelineTimings {
    /// 画像加载。
    pub profile_ms: f64,
    /// Prompt 拼装。
    pub prompt_ms: f64,
    /// 缓存查询。
    pub cache_lookup_ms: f64,
    /// 模型推理。
    pub model_ms: f64,
    /// HTML 校验。
    pub validation_ms: f64,
    /// 输出混淆。
    pub obfuscation_ms: f64,
    /// 兜底页预渲染。
    pub fallback_prepare_ms: f64,
    /// 端到端总耗时。
    pub total_ms: f64,
}

/// 管线输出。
#[derive(Debug, Clone, Serialize)]
pub struct PipelineOutcome {
    /// 渲染类型，固定 `SINGLE_FILE_HTML`。
    pub render_type: String,
    /// 渲染策略：`dynamic` 或 `template`。
    pub strategy: String,
    /// 可追溯版本串。
    pub version: String,
    /// 单文件 HTML。
    pub html_content: String,
    /// 是否发生降级。
    pub degraded: bool,
    /// 降级原因。
    pub degradation_reason: Option<DegradationReason>,
    /// 降级详情（已截断，便于排查）。
    pub degradation_detail: Option<String>,
    /// 是否命中缓存。
    pub cache_hit: bool,
    /// 实际使用的模型（降级时为空）。
    pub model: Option<String>,
    /// Prompt 稳定指纹（降级时为空）。
    pub prompt_hash: Option<String>,
    /// token 用量（降级时为空）。
    pub usage: Option<Usage>,
    /// 估算成本（美元）。
    pub estimated_cost_usd: f64,
    /// 模型尝试次数（含重试 / 备用模型）。
    pub attempts: u32,
    /// 分阶段耗时。
    pub timings: PipelineTimings,
    /// 是否执行了输出混淆。
    pub obfuscated: bool,
}

/// 管线运行信息。
#[derive(Debug, Clone, Serialize)]
pub struct PipelineInfo {
    /// 是否启用。
    pub enabled: bool,
    /// 端到端预算（毫秒）。
    pub timeout_ms: u64,
    /// 动态阶段预算（毫秒）。
    pub dynamic_budget_ms: u64,
    /// 兜底预留（毫秒）。
    pub fallback_reserve_ms: u64,
    /// 缓存是否启用。
    pub cache_enabled: bool,
    /// 缓存 TTL（秒）。
    pub cache_ttl_secs: u64,
    /// 缓存容量。
    pub cache_max_entries: usize,
    /// 混淆强度。
    pub obfuscation_mode: String,
    /// 版本策略说明。
    pub version_strategy: &'static str,
}

/// 流式首屏：预渲染的静态兜底页。
#[derive(Debug, Clone, Serialize)]
pub struct FallbackPreview {
    /// 渲染类型，固定 `SINGLE_FILE_HTML`。
    pub render_type: String,
    /// 渲染策略，固定 `template`。
    pub strategy: String,
    /// 可追溯版本串 `template:<scene>:<template_version>`。
    pub version: String,
    /// 完整单文件 HTML。
    pub html_content: String,
    /// 模板渲染耗时（毫秒）。
    pub render_ms: f64,
}

/// SSE 流式事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum PipelineStreamEvent {
    /// 首屏静态兜底页（连接建立后立即下发，保证不白屏）。
    Fallback {
        /// 兜底页。
        preview: FallbackPreview,
    },
    /// 动态生成成功（客户端可用它替换首屏兜底页）。
    Result {
        /// 完整管线输出。
        outcome: PipelineOutcome,
    },
    /// 动态生成失败并降级，`outcome.html_content` 即首屏兜底页。
    Degraded {
        /// 完整管线输出。
        outcome: PipelineOutcome,
    },
    /// 兜底页已下发、但后续流程出现无法降级的内部错误。
    Error {
        /// 错误码。
        code: i32,
        /// 错误说明。
        message: String,
    },
}

/// 管线错误（无法产出任何页面时返回）。
#[derive(Debug, Error)]
pub enum PipelineError {
    /// 未知场景。
    #[error("scene not found: {0}")]
    SceneNotFound(String),
    /// 请求参数不合法。
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// 业务数据不符合场景契约。
    #[error("invalid business_data: {0}")]
    InvalidBusinessData(String),
    /// 兜底模板渲染失败。
    #[error("fallback render failed: {0}")]
    FallbackFailed(String),
    /// 内部错误。
    #[error("internal pipeline error: {0}")]
    Internal(String),
}

/// 动态生成管线。
#[derive(Clone)]
pub struct Pipeline {
    config: PipelineConfig,
    model: Arc<ModelGateway>,
    db: Arc<Db>,
    metrics: Arc<PipelineMetrics>,
    cache: Option<Arc<ResponseCache>>,
    obfuscator: Arc<dyn HtmlObfuscator>,
}

impl Pipeline {
    /// 使用默认（无操作）混淆器构造管线。
    pub fn new(config: PipelineConfig, model: Arc<ModelGateway>, db: Arc<Db>) -> Self {
        Self::with_obfuscator(config, model, db, Arc::new(NoopObfuscator))
    }

    /// 注入自定义混淆器构造管线。
    pub fn with_obfuscator(
        config: PipelineConfig,
        model: Arc<ModelGateway>,
        db: Arc<Db>,
        obfuscator: Arc<dyn HtmlObfuscator>,
    ) -> Self {
        let cache = config.cache_enabled.then(|| {
            Arc::new(ResponseCache::new(
                config.cache_ttl_secs,
                config.cache_max_entries,
            ))
        });
        let metrics = Arc::new(PipelineMetrics::new(config.timeout_ms));
        Self {
            config,
            model,
            db,
            metrics,
            cache,
            obfuscator,
        }
    }

    /// 指标句柄。
    pub fn metrics(&self) -> &Arc<PipelineMetrics> {
        &self.metrics
    }

    /// 运行信息。
    pub fn info(&self) -> PipelineInfo {
        PipelineInfo {
            enabled: self.config.enabled,
            timeout_ms: self.config.timeout_ms,
            dynamic_budget_ms: self.config.dynamic_budget_ms(),
            fallback_reserve_ms: self.config.fallback_reserve_ms,
            cache_enabled: self.config.cache_enabled,
            cache_ttl_secs: self.config.cache_ttl_secs,
            cache_max_entries: self.config.cache_max_entries,
            obfuscation_mode: self.config.obfuscation_mode.to_string(),
            version_strategy: "dynamic:<scene>:<prompt_version>-<prompt_hash8>; degraded: template:<scene>:<template_version>",
        }
    }

    /// 校验场景 / 设备 / 业务数据并预渲染静态兜底页。
    fn prepare_fallback(
        &self,
        request: &PipelineRequest,
    ) -> Result<(&'static SceneDefinition, RenderedPage, f64), PipelineError> {
        let scene = REGISTRY
            .get(&request.scene_code)
            .ok_or_else(|| PipelineError::SceneNotFound(request.scene_code.clone()))?;
        validate_device(&request.device)?;
        validate_business_data(&request.business_data, scene.config.max_business_data_bytes)?;

        // 先渲染兜底页：既校验业务数据契约，也保证后续任何失败都能立即返回页面。
        let fallback_started = Instant::now();
        let fallback = TemplateEngine::global()
            .render_fallback(&request.scene_code, &request.business_data, &request.device)
            .map_err(|error| {
                self.metrics.record_fallback_failure();
                map_template_error(error)
            })?;
        Ok((scene, fallback, elapsed_ms(fallback_started)))
    }

    /// 预渲染流式首屏兜底页；不触发模型调用，供 SSE 端点立即下发。
    pub fn render_fallback_preview(
        &self,
        request: &PipelineRequest,
    ) -> Result<FallbackPreview, PipelineError> {
        let (_, fallback, render_ms) = self.prepare_fallback(request)?;
        // 首屏兜底页同样执行输出混淆；失败时退回原始模板，保证仍有页面。
        let html = match self.obfuscate(&fallback.html, None) {
            Ok((html, _, _)) => html,
            Err(error) => {
                tracing::warn!(error = %error, "fallback preview obfuscation failed");
                fallback.html
            }
        };
        Ok(FallbackPreview {
            render_type: "SINGLE_FILE_HTML".to_string(),
            strategy: "template".to_string(),
            version: format!(
                "template:{}:{}",
                fallback.scene_code, fallback.template_version
            ),
            html_content: html,
            render_ms,
        })
    }

    /// 执行一次生成。
    pub async fn generate(
        &self,
        request: PipelineRequest,
    ) -> Result<PipelineOutcome, PipelineError> {
        let started = Instant::now();
        let (scene, fallback, fallback_prepare_ms) = self.prepare_fallback(&request)?;

        if !self.config.enabled {
            return Ok(self.degraded_outcome(
                &fallback,
                DegradationReason::PipelineDisabled,
                None,
                PipelineTimings {
                    fallback_prepare_ms,
                    total_ms: elapsed_ms(started),
                    ..PipelineTimings::default()
                },
                false,
            ));
        }

        let profile_started = Instant::now();
        let (profile, profile_version) = self.load_profile(&request.user_id).await;
        let profile_ms = elapsed_ms(profile_started);

        let prompt_started = Instant::now();
        let constraints = scene_constraints(&request.scene_code, scene.config.display_name);
        let assembled =
            PromptAssembler::assemble(&constraints, &profile, &request.business_data, &[])
                .map_err(map_prompt_error)?;
        let prompt_ms = elapsed_ms(prompt_started);

        let user_prompt = request.prompt.clone().unwrap_or_else(|| {
            format!(
                "请生成「{}」场景的单文件 HTML 页面。",
                scene.config.display_name
            )
        });
        let cache_key = cache_key(&request, &profile_version, &assembled.prompt_hash);
        let mut timings = PipelineTimings {
            profile_ms,
            prompt_ms,
            fallback_prepare_ms,
            ..PipelineTimings::default()
        };

        if !request.bypass_cache {
            let lookup_started = Instant::now();
            let cached = self.cache.as_ref().and_then(|cache| cache.get(&cache_key));
            timings.cache_lookup_ms = elapsed_ms(lookup_started);
            if let Some(cached) = cached {
                return self.outcome_from_cache(cached, &fallback, &request, timings, started);
            }
        }

        let remaining = self
            .config
            .dynamic_budget_ms()
            .saturating_sub(started.elapsed().as_millis() as u64);
        if remaining == 0 {
            timings.total_ms = elapsed_ms(started);
            return Ok(self.degraded_outcome(
                &fallback,
                DegradationReason::DeadlineExceeded,
                None,
                timings,
                false,
            ));
        }

        let model_started = Instant::now();
        let model_future = self.model.generate_ui_with_system(
            UiRequest {
                prompt: user_prompt,
                scene_code: Some(request.scene_code.clone()),
                model: request.model.clone(),
                max_tokens: None,
                temperature: None,
            },
            Some(&assembled.system_prompt),
        );
        let model_result =
            tokio::time::timeout(Duration::from_millis(remaining), model_future).await;
        timings.model_ms = elapsed_ms(model_started);

        let generation = match model_result {
            Err(_) => {
                timings.total_ms = elapsed_ms(started);
                return Ok(self.degraded_outcome(
                    &fallback,
                    DegradationReason::ModelTimeout,
                    Some(format!("model exceeded {remaining}ms dynamic budget")),
                    timings,
                    false,
                ));
            }
            Ok(Err(error)) => {
                timings.total_ms = elapsed_ms(started);
                let reason = degradation_reason(&error);
                return Ok(self.degraded_outcome(
                    &fallback,
                    reason,
                    Some(truncate(&error.to_string(), 200)),
                    timings,
                    false,
                ));
            }
            Ok(Ok(generation)) => generation,
        };

        if started.elapsed() >= Duration::from_millis(self.config.timeout_ms) {
            timings.total_ms = elapsed_ms(started);
            return Ok(self.degraded_outcome(
                &fallback,
                DegradationReason::DeadlineExceeded,
                Some("dynamic generation exhausted the total budget".to_string()),
                timings,
                false,
            ));
        }

        let validation_started = Instant::now();
        let validation = validate_single_file_html(&generation.html);
        timings.validation_ms = elapsed_ms(validation_started);
        if !validation.valid {
            let detail = validation
                .first_error()
                .map(|issue| issue.message.clone())
                .unwrap_or_else(|| "unknown validation error".to_string());
            timings.total_ms = elapsed_ms(started);
            return Ok(self.degraded_outcome(
                &fallback,
                DegradationReason::ValidationFailed,
                Some(truncate(&detail, 200)),
                timings,
                false,
            ));
        }

        let (html, obfuscated, obfuscation_ms) =
            match self.obfuscate(&generation.html, request.session_id.as_deref()) {
                Ok(result) => result,
                Err(error) => {
                    timings.total_ms = elapsed_ms(started);
                    return Ok(self.degraded_outcome(
                        &fallback,
                        DegradationReason::ObfuscationFailed,
                        Some(truncate(&error, 200)),
                        timings,
                        false,
                    ));
                }
            };
        timings.obfuscation_ms = obfuscation_ms;

        let version = dynamic_version(&request.scene_code, &assembled.prompt_hash);
        if let Some(cache) = &self.cache {
            cache.insert(
                cache_key,
                cached_from_generation(&generation, &assembled.prompt_hash, version.clone()),
            );
        }

        timings.total_ms = elapsed_ms(started);
        let outcome = PipelineOutcome {
            render_type: "SINGLE_FILE_HTML".to_string(),
            strategy: "dynamic".to_string(),
            version,
            html_content: html,
            degraded: false,
            degradation_reason: None,
            degradation_detail: None,
            cache_hit: false,
            model: Some(generation.model.clone()),
            prompt_hash: Some(assembled.prompt_hash),
            usage: Some(generation.usage),
            estimated_cost_usd: generation.estimated_cost_usd,
            attempts: generation.attempts,
            timings,
            obfuscated,
        };
        self.metrics.record(&outcome);
        log_outcome(&request.scene_code, &outcome);
        Ok(outcome)
    }

    async fn load_profile(&self, user_id: &str) -> (PromptProfile, u64) {
        let repository = Repository::from_db(&self.db);
        match repository.find_user_profile(user_id).await {
            Ok(Some(profile)) => {
                let version = profile.version;
                (profile.prompt_profile(), version)
            }
            Ok(None) => (
                UserProfile::cold_start(user_id, Utc::now()).prompt_profile(),
                0,
            ),
            Err(error) => {
                tracing::warn!(
                    target: "pipeline",
                    user_id = %user_id,
                    error = %error,
                    "failed to load user profile; falling back to cold-start profile"
                );
                (
                    UserProfile::cold_start(user_id, Utc::now()).prompt_profile(),
                    0,
                )
            }
        }
    }

    fn obfuscate(
        &self,
        html: &str,
        session_id: Option<&str>,
    ) -> Result<(String, bool, f64), String> {
        if !self.config.obfuscation_mode.enabled() {
            return Ok((html.to_string(), false, 0.0));
        }
        let started = Instant::now();
        let output = self.obfuscator.obfuscate(html, session_id)?;
        Ok((output.html, output.applied, elapsed_ms(started)))
    }

    fn outcome_from_cache(
        &self,
        cached: CachedGeneration,
        fallback: &RenderedPage,
        request: &PipelineRequest,
        mut timings: PipelineTimings,
        started: Instant,
    ) -> Result<PipelineOutcome, PipelineError> {
        let (html, obfuscated, obfuscation_ms) =
            match self.obfuscate(&cached.html, request.session_id.as_deref()) {
                Ok(result) => result,
                Err(error) => {
                    timings.total_ms = elapsed_ms(started);
                    return Ok(self.degraded_outcome(
                        fallback,
                        DegradationReason::ObfuscationFailed,
                        Some(truncate(&error, 200)),
                        timings,
                        true,
                    ));
                }
            };
        timings.obfuscation_ms = obfuscation_ms;
        timings.total_ms = elapsed_ms(started);
        let outcome = PipelineOutcome {
            render_type: "SINGLE_FILE_HTML".to_string(),
            strategy: "dynamic".to_string(),
            version: cached.version,
            html_content: html,
            degraded: false,
            degradation_reason: None,
            degradation_detail: None,
            cache_hit: true,
            model: Some(cached.model),
            prompt_hash: Some(cached.prompt_hash),
            usage: Some(cached.usage),
            estimated_cost_usd: cached.estimated_cost_usd,
            attempts: 0,
            timings,
            obfuscated,
        };
        self.metrics.record(&outcome);
        log_outcome(&request.scene_code, &outcome);
        Ok(outcome)
    }

    fn degraded_outcome(
        &self,
        fallback: &RenderedPage,
        reason: DegradationReason,
        detail: Option<String>,
        timings: PipelineTimings,
        cache_hit: bool,
    ) -> PipelineOutcome {
        // 降级页也执行输出混淆；混淆失败时退回原始模板（优先保证不白屏）。
        let (html, obfuscated, obfuscation_ms) = match self.obfuscate(&fallback.html, None) {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(error = %error, "fallback obfuscation failed; serving raw template");
                (fallback.html.clone(), false, 0.0)
            }
        };
        let mut timings = timings;
        timings.obfuscation_ms = obfuscation_ms;
        timings.total_ms += obfuscation_ms;
        let outcome = PipelineOutcome {
            render_type: "SINGLE_FILE_HTML".to_string(),
            strategy: "template".to_string(),
            version: format!(
                "template:{}:{}",
                fallback.scene_code, fallback.template_version
            ),
            html_content: html,
            degraded: true,
            degradation_reason: Some(reason),
            degradation_detail: detail,
            cache_hit,
            model: None,
            prompt_hash: None,
            usage: None,
            estimated_cost_usd: 0.0,
            attempts: 0,
            timings,
            obfuscated,
        };
        self.metrics.record(&outcome);
        tracing::warn!(
            target: "pipeline",
            scene = %fallback.scene_code,
            reason = reason.as_str(),
            degraded = true,
            cache_hit = outcome.cache_hit,
            total_ms = outcome.timings.total_ms,
            "ui pipeline degraded to static template"
        );
        outcome
    }
}

fn validate_device(device: &DeviceInfo) -> Result<(), PipelineError> {
    if !(1..=20_000).contains(&device.screen_width) {
        return Err(PipelineError::InvalidRequest(
            "device_info.screen_width must be within 1..=20000".to_string(),
        ));
    }
    if !(1..=20_000).contains(&device.screen_height) {
        return Err(PipelineError::InvalidRequest(
            "device_info.screen_height must be within 1..=20000".to_string(),
        ));
    }
    if device.platform.trim().is_empty() || device.platform.chars().count() > 32 {
        return Err(PipelineError::InvalidRequest(
            "device_info.platform must be 1..=32 characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_business_data(business_data: &Value, max_bytes: usize) -> Result<(), PipelineError> {
    if !business_data.is_object() {
        return Err(PipelineError::InvalidBusinessData(
            "business_data must be a JSON object".to_string(),
        ));
    }
    let size = serde_json::to_vec(business_data)
        .map_err(|error| PipelineError::Internal(format!("serialize business_data: {error}")))?
        .len();
    if size > max_bytes {
        return Err(PipelineError::InvalidBusinessData(format!(
            "business_data must be at most {max_bytes} bytes (got {size})"
        )));
    }
    Ok(())
}

fn scene_constraints(scene_code: &str, display_name: &str) -> SceneConstraints {
    builtin_scene_constraints(scene_code).unwrap_or_else(|| SceneConstraints {
        scene_code: scene_code.to_string(),
        display_name: display_name.to_string(),
        required_elements: Vec::new(),
        allowed_actions: Vec::new(),
        forbidden: vec!["外部脚本、样式、字体或图片".to_string()],
        layout_notes: Vec::new(),
    })
}

fn cache_key(request: &PipelineRequest, profile_version: &u64, prompt_hash: &str) -> String {
    let business = serde_json::to_string(&request.business_data).unwrap_or_default();
    let mut hasher = Fnv1a64::new();
    for part in [
        request.scene_code.as_str(),
        request.user_id.as_str(),
        request.device.platform.as_str(),
        prompt_hash,
        business.as_str(),
        request.prompt.as_deref().unwrap_or(""),
        request.model.as_deref().unwrap_or(""),
    ] {
        hasher.write(part.as_bytes());
        hasher.write(&[0]);
    }
    hasher.write(&request.device.screen_width.to_le_bytes());
    hasher.write(&request.device.screen_height.to_le_bytes());
    hasher.write(&profile_version.to_le_bytes());
    format!("{:016x}", hasher.finish())
}

/// 动态版本：`dynamic:<scene>:<prompt_version>-<prompt_hash8>`。
pub fn dynamic_version(scene_code: &str, prompt_hash: &str) -> String {
    let short = &prompt_hash[..prompt_hash.len().min(8)];
    format!("dynamic:{scene_code}:{PROMPT_TEMPLATE_VERSION}-{short}")
}

fn degradation_reason(error: &ModelError) -> DegradationReason {
    match error {
        ModelError::Timeout => DegradationReason::ModelTimeout,
        ModelError::QueueFull | ModelError::QueueTimeout => DegradationReason::ModelBusy,
        ModelError::Disabled => DegradationReason::ModelDisabled,
        ModelError::InvalidOutput(_) => DegradationReason::ValidationFailed,
        _ => DegradationReason::ModelError,
    }
}

fn map_template_error(error: TemplateError) -> PipelineError {
    match error {
        TemplateError::SceneNotFound(message) => PipelineError::SceneNotFound(message),
        TemplateError::InvalidBusinessData(message) => PipelineError::InvalidBusinessData(message),
        TemplateError::Render(message) => PipelineError::FallbackFailed(message),
    }
}

fn map_prompt_error(error: PromptError) -> PipelineError {
    PipelineError::InvalidRequest(error.to_string())
}

fn log_outcome(scene_code: &str, outcome: &PipelineOutcome) {
    tracing::info!(
        target: "pipeline",
        scene = %scene_code,
        strategy = %outcome.strategy,
        version = %outcome.version,
        degraded = outcome.degraded,
        cache_hit = outcome.cache_hit,
        model = outcome.model.as_deref().unwrap_or("-"),
        total_ms = outcome.timings.total_ms,
        model_ms = outcome.timings.model_ms,
        validation_ms = outcome.timings.validation_ms,
        obfuscation_ms = outcome.timings.obfuscation_ms,
        "ui pipeline completed"
    );
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn elapsed_ms(started: Instant) -> f64 {
    (started.elapsed().as_secs_f64() * 1000.0 * 1000.0).round() / 1000.0
}

/// FNV-1a 64 位哈希（非加密用途，仅用于缓存键）。
struct Fnv1a64(u64);

impl Fnv1a64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// 便于测试与外部调用：把一次成功的模型结果转换为缓存条目。
pub(crate) fn cached_from_generation(
    generation: &UiGeneration,
    prompt_hash: &str,
    version: String,
) -> CachedGeneration {
    CachedGeneration {
        html: generation.html.clone(),
        model: generation.model.clone(),
        usage: generation.usage,
        estimated_cost_usd: generation.estimated_cost_usd,
        finish_reason: generation.finish_reason.clone(),
        prompt_hash: prompt_hash.to_string(),
        version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_version_is_content_addressed() {
        let version = dynamic_version("ORDER_DETAIL_PAGE", "0123456789abcdef");
        assert_eq!(version, "dynamic:ORDER_DETAIL_PAGE:v1-01234567");
    }

    #[test]
    fn degradation_reasons_are_stable() {
        assert_eq!(DegradationReason::ModelTimeout.as_str(), "model_timeout");
        assert_eq!(
            DegradationReason::ValidationFailed.as_str(),
            "validation_failed"
        );
    }
}
