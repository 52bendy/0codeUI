//! 模型网关：模型路由 / 灰度、并发与排队控制、超时重试、指标与输出校验。

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::stream::{self, BoxStream, StreamExt};
use serde::Serialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::config::ModelConfig;
use super::error::ModelError;
use super::metrics::{MetricsSnapshot, ModelMetrics};
use super::mock::MockProvider;
use super::openai::OpenAiCompatibleProvider;
use super::prompt::{build_messages, build_messages_with_system, extract_and_validate};
use super::provider::{ModelProvider, ModelStream};
use super::types::{ChatRequest, Usage};

/// 生成请求。
#[derive(Debug, Clone)]
pub struct UiRequest {
    /// 用户需求。
    pub prompt: String,
    /// 场景编码（可选）。
    pub scene_code: Option<String>,
    /// 显式指定模型（必须是已注册模型）。
    pub model: Option<String>,
    /// 最大输出 token。
    pub max_tokens: Option<u32>,
    /// 采样温度。
    pub temperature: Option<f32>,
}

/// 非流式生成结果。
#[derive(Debug, Clone, Serialize)]
pub struct UiGeneration {
    /// 单文件 HTML。
    pub html: String,
    /// 实际使用的模型。
    pub model: String,
    /// 端到端耗时（毫秒）。
    pub latency_ms: f64,
    /// 尝试次数（含重试与备用模型）。
    pub attempts: u32,
    /// token 用量。
    pub usage: Usage,
    /// 估算成本（美元）。
    pub estimated_cost_usd: f64,
    /// 结束原因。
    pub finish_reason: Option<String>,
}

/// 网关流式事件流。
pub type UiStream = BoxStream<'static, Result<StreamEvent, ModelError>>;

/// 流式事件。
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 元信息（首个事件）。
    Meta {
        /// 模型名。
        model: String,
    },
    /// 增量文本。
    Delta {
        /// 文本片段。
        content: String,
    },
    /// 结束（含用量与耗时）。
    Done {
        /// 模型名。
        model: String,
        /// token 用量。
        usage: Usage,
        /// 端到端耗时（毫秒）。
        latency_ms: f64,
        /// 估算成本（美元）。
        estimated_cost_usd: f64,
        /// 结束原因。
        finish_reason: Option<String>,
    },
}

/// 网关运行信息。
#[derive(Debug, Clone, Serialize)]
pub struct GatewayInfo {
    /// 提供者名称。
    pub provider: &'static str,
    /// 是否启用。
    pub enabled: bool,
    /// 主模型。
    pub primary_model: String,
    /// 全部已注册模型（主模型 + 备用 + 灰度）。
    pub models: Vec<String>,
    /// 灰度模型。
    pub canary_model: Option<String>,
    /// 灰度百分比。
    pub canary_percent: u8,
    /// 备用模型。
    pub fallback_models: Vec<String>,
    /// 最大并发。
    pub max_concurrency: usize,
    /// 最大排队。
    pub max_queue: usize,
    /// 非流式超时（毫秒）。
    pub request_timeout_ms: u64,
    /// 流式超时（毫秒）。
    pub stream_timeout_ms: u64,
    /// 最大重试次数。
    pub max_retries: u32,
}

/// 模型网关。
pub struct ModelGateway {
    config: ModelConfig,
    provider: Arc<dyn ModelProvider>,
    semaphore: Arc<Semaphore>,
    waiting: Arc<AtomicUsize>,
    metrics: Arc<ModelMetrics>,
    route_counter: Arc<AtomicU64>,
}

impl ModelGateway {
    /// 根据配置创建真实提供者。
    pub fn from_config(config: &ModelConfig) -> Result<Self, ModelError> {
        let provider: Arc<dyn ModelProvider> = match config.provider {
            super::config::ProviderKind::Mock => Arc::new(MockProvider::new(
                config.mock_latency_ms,
                config.mock_stream_delay_ms,
            )),
            super::config::ProviderKind::OpenAiCompatible => {
                Arc::new(OpenAiCompatibleProvider::new(config)?)
            }
        };
        Ok(Self::new(config.clone(), provider))
    }

    /// 注入自定义提供者（测试 / 联调）。
    pub fn new(config: ModelConfig, provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrency)),
            config,
            provider,
            waiting: Arc::new(AtomicUsize::new(0)),
            metrics: Arc::new(ModelMetrics::default()),
            route_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 配置。
    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    /// 指标。
    pub fn metrics(&self) -> &ModelMetrics {
        &self.metrics
    }

    /// 指标快照。
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// 运行信息。
    pub fn info(&self) -> GatewayInfo {
        GatewayInfo {
            provider: self.provider.name(),
            enabled: self.config.enabled,
            primary_model: self.config.primary_model.clone(),
            models: self.config.allowed_models(),
            canary_model: self.config.canary_model.clone(),
            canary_percent: self.config.canary_percent,
            fallback_models: self.config.fallback_models.clone(),
            max_concurrency: self.config.max_concurrency,
            max_queue: self.config.max_queue,
            request_timeout_ms: self.config.request_timeout_ms,
            stream_timeout_ms: self.config.stream_timeout_ms,
            max_retries: self.config.max_retries,
        }
    }

    /// 非流式生成 UI。
    pub async fn generate_ui(&self, request: UiRequest) -> Result<UiGeneration, ModelError> {
        self.generate_ui_with_system(request, None).await
    }

    /// 使用自定义 System Prompt 非流式生成 UI（动态管线使用）。
    pub async fn generate_ui_with_system(
        &self,
        request: UiRequest,
        system_prompt: Option<&str>,
    ) -> Result<UiGeneration, ModelError> {
        let validated = ValidatedRequest::new(&self.config, request)?;
        let _permit = self.acquire().await?;
        let selected = self.select_model(validated.model.as_deref())?;
        let chain = self.model_chain(&selected);
        let messages = match system_prompt {
            Some(system_prompt) => build_messages_with_system(
                system_prompt,
                &validated.prompt,
                validated.scene_code.as_deref(),
            ),
            None => build_messages(&validated.prompt, validated.scene_code.as_deref()),
        };
        let mut last_error = None;
        let mut attempts = 0u32;

        for model in chain {
            match self
                .try_model(&model, &messages, &validated, &mut attempts)
                .await
            {
                Ok(generation) => return Ok(generation),
                Err(error) => {
                    if !error.is_retryable() {
                        return Err(error);
                    }
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| ModelError::Upstream {
            status: 0,
            message: "no model available".to_string(),
            retryable: false,
        }))
    }

    async fn try_model(
        &self,
        model: &str,
        messages: &[super::types::ChatMessage],
        request: &ValidatedRequest,
        attempts: &mut u32,
    ) -> Result<UiGeneration, ModelError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                self.metrics.record_retry();
                self.backoff(attempt).await;
            }
            *attempts += 1;
            let started = Instant::now();
            let chat = ChatRequest {
                model: model.to_string(),
                messages: messages.to_vec(),
                max_tokens: request.max_tokens,
                temperature: request.temperature,
                stream: false,
            };
            match self.provider.generate(chat).await {
                Ok(response) => {
                    let latency = elapsed_ms(started);
                    match extract_and_validate(&response.content) {
                        Ok(html) => {
                            let usage =
                                normalize_usage(response.usage, &request.prompt, &response.content);
                            let cost = self.estimate_cost(&usage);
                            self.metrics.record(model, latency, true, usage, cost);
                            return Ok(UiGeneration {
                                html,
                                model: model.to_string(),
                                latency_ms: round3(latency),
                                attempts: *attempts,
                                usage,
                                estimated_cost_usd: round6(cost),
                                finish_reason: response.finish_reason,
                            });
                        }
                        Err(error) => {
                            self.metrics
                                .record(model, latency, false, response.usage, 0.0);
                            return Err(error);
                        }
                    }
                }
                Err(error) => {
                    let latency = elapsed_ms(started);
                    self.metrics
                        .record(model, latency, false, Usage::default(), 0.0);
                    if !error.is_retryable() || attempt == self.config.max_retries {
                        return Err(error);
                    }
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or(ModelError::Timeout))
    }

    /// 流式生成 UI。
    pub async fn stream_ui(&self, request: UiRequest) -> Result<UiStream, ModelError> {
        self.stream_ui_with_system(request, None).await
    }

    /// 使用自定义 System Prompt 流式生成 UI（动态管线使用）。
    pub async fn stream_ui_with_system(
        &self,
        request: UiRequest,
        system_prompt: Option<&str>,
    ) -> Result<UiStream, ModelError> {
        let validated = ValidatedRequest::new(&self.config, request)?;
        let permit = self.acquire().await?;
        let selected = self.select_model(validated.model.as_deref())?;
        let messages = match system_prompt {
            Some(system_prompt) => build_messages_with_system(
                system_prompt,
                &validated.prompt,
                validated.scene_code.as_deref(),
            ),
            None => build_messages(&validated.prompt, validated.scene_code.as_deref()),
        };
        let chat = ChatRequest {
            model: selected.clone(),
            messages,
            max_tokens: validated.max_tokens,
            temperature: validated.temperature,
            stream: true,
        };
        let inner = match self.provider.generate_stream(chat).await {
            Ok(stream) => stream,
            Err(error) => {
                self.metrics
                    .record(&selected, 0.0, false, Usage::default(), 0.0);
                return Err(error);
            }
        };
        Ok(self.wrap_stream(selected, inner, permit, Instant::now()))
    }

    fn wrap_stream(
        &self,
        model: String,
        inner: ModelStream,
        permit: OwnedSemaphorePermit,
        started: Instant,
    ) -> UiStream {
        let state = StreamState {
            inner,
            _permit: permit,
            metrics: self.metrics.clone(),
            model,
            started,
            usage: Usage::default(),
            finish_reason: None,
            meta_sent: false,
            finished: false,
            price_input: self.config.price_input_per_1k,
            price_output: self.config.price_output_per_1k,
        };
        let stream = stream::unfold(state, |mut state| async move {
            if state.finished {
                return None;
            }
            if !state.meta_sent {
                state.meta_sent = true;
                return Some((
                    Ok(StreamEvent::Meta {
                        model: state.model.clone(),
                    }),
                    state,
                ));
            }
            loop {
                match state.inner.next().await {
                    Some(Ok(chunk)) => {
                        if let Some(usage) = chunk.usage {
                            state.usage.merge(usage);
                        }
                        if chunk.finish_reason.is_some() {
                            state.finish_reason = chunk.finish_reason;
                        }
                        if let Some(delta) = chunk.delta {
                            if !delta.is_empty() {
                                return Some((Ok(StreamEvent::Delta { content: delta }), state));
                            }
                        }
                    }
                    Some(Err(error)) => {
                        state.finished = true;
                        let latency = elapsed_ms(state.started);
                        state
                            .metrics
                            .record(&state.model, latency, false, state.usage, 0.0);
                        return Some((Err(error), state));
                    }
                    None => {
                        state.finished = true;
                        let latency = elapsed_ms(state.started);
                        let cost = cost_of(state.usage, state.price_input, state.price_output);
                        state
                            .metrics
                            .record(&state.model, latency, true, state.usage, cost);
                        return Some((
                            Ok(StreamEvent::Done {
                                model: state.model.clone(),
                                usage: state.usage,
                                latency_ms: round3(latency),
                                estimated_cost_usd: round6(cost),
                                finish_reason: state.finish_reason.take(),
                            }),
                            state,
                        ));
                    }
                }
            }
        });
        Box::pin(stream)
    }

    async fn acquire(&self) -> Result<OwnedSemaphorePermit, ModelError> {
        if !self.config.enabled {
            return Err(ModelError::Disabled);
        }
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            return Ok(permit);
        }
        let waiting = self.waiting.fetch_add(1, Ordering::SeqCst) + 1;
        if waiting > self.config.max_queue {
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            self.metrics.record_queue_rejection();
            return Err(ModelError::QueueFull);
        }
        let result = tokio::time::timeout(
            Duration::from_millis(self.config.queue_timeout_ms),
            self.semaphore.clone().acquire_owned(),
        )
        .await;
        self.waiting.fetch_sub(1, Ordering::SeqCst);
        match result {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_)) => Err(ModelError::QueueTimeout),
            Err(_) => Err(ModelError::QueueTimeout),
        }
    }

    fn select_model(&self, requested: Option<&str>) -> Result<String, ModelError> {
        if let Some(model) = requested {
            if !self
                .config
                .allowed_models()
                .iter()
                .any(|item| item == model)
            {
                return Err(ModelError::UnknownModel(model.to_string()));
            }
            return Ok(model.to_string());
        }
        let counter = self.route_counter.fetch_add(1, Ordering::Relaxed);
        if self.config.canary_percent > 0 {
            if let Some(canary) = &self.config.canary_model {
                if counter % 100 < u64::from(self.config.canary_percent) {
                    return Ok(canary.clone());
                }
            }
        }
        Ok(self.config.primary_model.clone())
    }

    fn model_chain(&self, selected: &str) -> Vec<String> {
        let mut chain = vec![selected.to_string()];
        for model in std::iter::once(&self.config.primary_model).chain(&self.config.fallback_models)
        {
            if !chain.contains(model) {
                chain.push(model.clone());
            }
        }
        chain
    }

    fn estimate_cost(&self, usage: &Usage) -> f64 {
        cost_of(
            *usage,
            self.config.price_input_per_1k,
            self.config.price_output_per_1k,
        )
    }

    async fn backoff(&self, attempt: u32) {
        let multiplier = 1u64 << attempt.saturating_sub(1).min(5);
        let delay = self.config.retry_backoff_ms.saturating_mul(multiplier);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }
}

struct ValidatedRequest {
    prompt: String,
    scene_code: Option<String>,
    model: Option<String>,
    max_tokens: u32,
    temperature: f32,
}

impl ValidatedRequest {
    fn new(config: &ModelConfig, request: UiRequest) -> Result<Self, ModelError> {
        let prompt = request.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err(ModelError::InvalidRequest(
                "prompt must not be empty".to_string(),
            ));
        }
        if prompt.chars().count() > 32_000 || prompt.len() > 128 * 1024 {
            return Err(ModelError::InvalidRequest(
                "prompt exceeds 32000 characters / 128 KiB".to_string(),
            ));
        }
        if let Some(scene) = request.scene_code.as_deref() {
            validate_scene_code(scene)?;
        }
        let max_tokens = request.max_tokens.unwrap_or(config.max_tokens);
        if max_tokens == 0 || max_tokens > 8192 {
            return Err(ModelError::InvalidRequest(
                "max_tokens must be in 1..=8192".to_string(),
            ));
        }
        let temperature = request.temperature.unwrap_or(config.temperature);
        if !(temperature.is_finite() && (0.0..=2.0).contains(&temperature)) {
            return Err(ModelError::InvalidRequest(
                "temperature must be in 0..=2".to_string(),
            ));
        }
        Ok(Self {
            prompt,
            scene_code: request.scene_code,
            model: request.model,
            max_tokens,
            temperature,
        })
    }
}

fn validate_scene_code(value: &str) -> Result<(), ModelError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'));
    if valid {
        Ok(())
    } else {
        Err(ModelError::InvalidRequest(
            "scene_code must match [A-Za-z0-9._:-]{1,128}".to_string(),
        ))
    }
}

fn normalize_usage(usage: Usage, prompt: &str, output: &str) -> Usage {
    if usage.total_tokens > 0 {
        return usage;
    }
    let prompt_tokens = (prompt.chars().count() / 4).max(1) as u32;
    let completion_tokens = (output.chars().count() / 4).max(1) as u32;
    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
    }
}

fn cost_of(usage: Usage, price_input: f64, price_output: f64) -> f64 {
    let input = f64::from(usage.prompt_tokens) / 1000.0 * price_input;
    let output = f64::from(usage.completion_tokens) / 1000.0 * price_output;
    input + output
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

struct StreamState {
    inner: ModelStream,
    _permit: OwnedSemaphorePermit,
    metrics: Arc<ModelMetrics>,
    model: String,
    started: Instant,
    usage: Usage,
    finish_reason: Option<String>,
    meta_sent: bool,
    finished: bool,
    price_input: f64,
    price_output: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{ChatResponse, StreamChunk};
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;

    struct CountingProvider {
        active: AtomicUsize,
        max_active: AtomicUsize,
        delay_ms: u64,
    }

    #[async_trait]
    impl ModelProvider for CountingProvider {
        fn name(&self) -> &'static str {
            "counting"
        }

        async fn generate(&self, _request: ChatRequest) -> Result<ChatResponse, ModelError> {
            let current = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(current, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(ChatResponse {
                content: "<!DOCTYPE html><html><head></head><body>ok</body></html>".to_string(),
                finish_reason: Some("stop".to_string()),
                usage: Usage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
            })
        }

        async fn generate_stream(&self, _request: ChatRequest) -> Result<ModelStream, ModelError> {
            Ok(Box::pin(stream::iter(vec![Ok(StreamChunk {
                delta: Some("ok".to_string()),
                finish_reason: Some("stop".to_string()),
                usage: Some(Usage::default()),
            })])))
        }
    }

    fn config(max_concurrency: usize) -> ModelConfig {
        ModelConfig {
            max_concurrency,
            max_queue: 2,
            queue_timeout_ms: 200,
            max_retries: 0,
            ..ModelConfig::default()
        }
    }

    #[tokio::test]
    async fn gateway_limits_concurrency() {
        let provider = Arc::new(CountingProvider {
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            delay_ms: 30,
        });
        let gateway = Arc::new(ModelGateway::new(config(1), provider.clone()));
        let mut handles = Vec::new();
        for _ in 0..3 {
            let gateway = gateway.clone();
            handles.push(tokio::spawn(async move {
                gateway
                    .generate_ui(UiRequest {
                        prompt: "生成页面".to_string(),
                        scene_code: None,
                        model: None,
                        max_tokens: None,
                        temperature: None,
                    })
                    .await
            }));
        }
        for handle in handles {
            assert!(handle.await.unwrap().is_ok());
        }
        assert_eq!(provider.max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn gateway_rejects_unknown_model() {
        let provider = Arc::new(CountingProvider {
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            delay_ms: 0,
        });
        let gateway = ModelGateway::new(config(1), provider);
        let error = gateway
            .generate_ui(UiRequest {
                prompt: "x".to_string(),
                scene_code: None,
                model: Some("not-registered".to_string()),
                max_tokens: None,
                temperature: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, ModelError::UnknownModel(_)));
    }

    #[tokio::test]
    async fn canary_percent_routes_to_canary() {
        let provider = Arc::new(CountingProvider {
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            delay_ms: 0,
        });
        let mut config = config(2);
        config.canary_model = Some("canary-model".to_string());
        config.canary_percent = 100;
        let gateway = ModelGateway::new(config, provider);
        let result = gateway
            .generate_ui(UiRequest {
                prompt: "x".to_string(),
                scene_code: None,
                model: None,
                max_tokens: None,
                temperature: None,
            })
            .await
            .unwrap();
        assert_eq!(result.model, "canary-model");
    }
}
