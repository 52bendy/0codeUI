//! UI 生成模型接入配置（`MODEL_*` 环境变量）。
//!
//! 默认面向本机 / 内网 vLLM、Ollama 等 OpenAI 兼容推理服务；
//! 开发环境默认使用确定性的 `mock` 提供者，便于无 GPU 环境联调与 CI。

use std::fmt;
use std::str::FromStr;

use crate::config::{env_parse, env_string, optional_env, parse_bool, ConfigError};

/// 推理提供者类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// 确定性 mock（仅开发 / 测试，禁止用于生产）。
    Mock,
    /// OpenAI 兼容接口（vLLM / Ollama / DeepSeek 等）。
    OpenAiCompatible,
}

impl ProviderKind {
    /// 是否为 mock。
    pub const fn is_mock(self) -> bool {
        matches!(self, Self::Mock)
    }

    /// 配置值名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }
}

impl FromStr for ProviderKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mock" => Ok(Self::Mock),
            "openai" | "openai_compatible" | "vllm" | "ollama" => Ok(Self::OpenAiCompatible),
            other => Err(format!(
                "expected `mock` or `openai_compatible`, got `{other}`"
            )),
        }
    }
}

/// 敏感字符串：`Debug` 输出自动脱敏，避免密钥进入日志。
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// 由明文构造。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 读取明文（仅在构造请求头时调用）。
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("SecretString(empty)")
        } else {
            f.write_str("SecretString(***)")
        }
    }
}

/// 模型接入与推理部署配置。
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// 是否启用模型接口：`MODEL_ENABLED`（默认 true）。
    pub enabled: bool,
    /// 提供者：`MODEL_PROVIDER`（默认 mock）。
    pub provider: ProviderKind,
    /// OpenAI 兼容服务地址（不含 `/chat/completions`）：`MODEL_BASE_URL`。
    pub base_url: String,
    /// 服务端密钥：`MODEL_API_KEY`（本机 vLLM 可为空）。
    pub api_key: SecretString,
    /// 主模型名：`MODEL_NAME`。
    pub primary_model: String,
    /// 备用模型链（逗号分隔）：`MODEL_FALLBACKS`。
    pub fallback_models: Vec<String>,
    /// 灰度候选模型：`MODEL_CANARY_MODEL`（可空）。
    pub canary_model: Option<String>,
    /// 灰度流量百分比（0–100）：`MODEL_CANARY_PERCENT`。
    pub canary_percent: u8,
    /// 非流式请求总超时（毫秒）：`MODEL_REQUEST_TIMEOUT_MS`（默认 1500）。
    pub request_timeout_ms: u64,
    /// 流式分片读取超时（空闲超时，毫秒）：`MODEL_STREAM_TIMEOUT_MS`（默认 30000）。
    pub stream_timeout_ms: u64,
    /// 建连超时（毫秒）：`MODEL_CONNECT_TIMEOUT_MS`（默认 300）。
    pub connect_timeout_ms: u64,
    /// 最大并发推理数：`MODEL_MAX_CONCURRENCY`（默认 4）。
    pub max_concurrency: usize,
    /// 最大排队数：`MODEL_MAX_QUEUE`（默认 64）。
    pub max_queue: usize,
    /// 排队等待上限（毫秒）：`MODEL_QUEUE_TIMEOUT_MS`（默认 1500）。
    pub queue_timeout_ms: u64,
    /// 失败重试次数：`MODEL_MAX_RETRIES`（默认 1）。
    pub max_retries: u32,
    /// 重试退避基数（毫秒）：`MODEL_RETRY_BACKOFF_MS`（默认 100）。
    pub retry_backoff_ms: u64,
    /// 默认最大输出 token：`MODEL_MAX_TOKENS`（默认 2048）。
    pub max_tokens: u32,
    /// 默认温度：`MODEL_TEMPERATURE`（默认 0.2）。
    pub temperature: f32,
    /// 输入价格（美元 / 千 token）：`MODEL_PRICE_INPUT_PER_1K`（默认 0，自托管）。
    pub price_input_per_1k: f64,
    /// 输出价格（美元 / 千 token）：`MODEL_PRICE_OUTPUT_PER_1K`（默认 0，自托管）。
    pub price_output_per_1k: f64,
    /// mock 单次延迟（毫秒）：`MODEL_MOCK_LATENCY_MS`。
    pub mock_latency_ms: u64,
    /// mock 流式分片间隔（毫秒）：`MODEL_MOCK_STREAM_DELAY_MS`。
    pub mock_stream_delay_ms: u64,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: ProviderKind::Mock,
            base_url: "http://127.0.0.1:8000/v1".to_string(),
            api_key: SecretString::new(""),
            primary_model: "Qwen2.5-Coder-7B-Instruct-AWQ".to_string(),
            fallback_models: Vec::new(),
            canary_model: None,
            canary_percent: 0,
            request_timeout_ms: 1500,
            stream_timeout_ms: 30_000,
            connect_timeout_ms: 300,
            max_concurrency: 4,
            max_queue: 64,
            queue_timeout_ms: 1500,
            max_retries: 1,
            retry_backoff_ms: 100,
            max_tokens: 2048,
            temperature: 0.2,
            price_input_per_1k: 0.0,
            price_output_per_1k: 0.0,
            mock_latency_ms: 5,
            mock_stream_delay_ms: 2,
        }
    }
}

impl ModelConfig {
    /// 从环境变量加载并校验模型配置。
    pub fn from_env() -> Result<Self, ConfigError> {
        let provider = match optional_env("MODEL_PROVIDER") {
            Some(value) => ProviderKind::from_str(&value).map_err(|message| ConfigError {
                key: "MODEL_PROVIDER",
                message,
            })?,
            None => ProviderKind::Mock,
        };

        let fallback_models = optional_env("MODEL_FALLBACKS")
            .map(|value| parse_list(&value))
            .unwrap_or_default();
        let canary_model = optional_env("MODEL_CANARY_MODEL");
        let canary_percent = env_parse("MODEL_CANARY_PERCENT", 0u8)?;

        let config = Self {
            enabled: match optional_env("MODEL_ENABLED") {
                Some(value) => parse_bool("MODEL_ENABLED", &value)?,
                None => true,
            },
            provider,
            base_url: env_string("MODEL_BASE_URL", "http://127.0.0.1:8000/v1"),
            api_key: SecretString::new(env_string("MODEL_API_KEY", "")),
            primary_model: env_string("MODEL_NAME", "Qwen2.5-Coder-7B-Instruct-AWQ"),
            fallback_models,
            canary_model,
            canary_percent,
            request_timeout_ms: env_parse("MODEL_REQUEST_TIMEOUT_MS", 1500u64)?,
            stream_timeout_ms: env_parse("MODEL_STREAM_TIMEOUT_MS", 30_000u64)?,
            connect_timeout_ms: env_parse("MODEL_CONNECT_TIMEOUT_MS", 300u64)?,
            max_concurrency: env_parse("MODEL_MAX_CONCURRENCY", 4usize)?,
            max_queue: env_parse("MODEL_MAX_QUEUE", 64usize)?,
            queue_timeout_ms: env_parse("MODEL_QUEUE_TIMEOUT_MS", 1500u64)?,
            max_retries: env_parse("MODEL_MAX_RETRIES", 1u32)?,
            retry_backoff_ms: env_parse("MODEL_RETRY_BACKOFF_MS", 100u64)?,
            max_tokens: env_parse("MODEL_MAX_TOKENS", 2048u32)?,
            temperature: env_parse("MODEL_TEMPERATURE", 0.2f32)?,
            price_input_per_1k: env_parse("MODEL_PRICE_INPUT_PER_1K", 0.0f64)?,
            price_output_per_1k: env_parse("MODEL_PRICE_OUTPUT_PER_1K", 0.0f64)?,
            mock_latency_ms: env_parse("MODEL_MOCK_LATENCY_MS", 5u64)?,
            mock_stream_delay_ms: env_parse("MODEL_MOCK_STREAM_DELAY_MS", 2u64)?,
        };
        config.validate()?;
        Ok(config)
    }

    /// 校验取值边界，避免错误配置在运行时才暴露。
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.primary_model.trim().is_empty() {
            return Err(invalid("MODEL_NAME", "must not be empty"));
        }
        if self.max_concurrency == 0 || self.max_concurrency > 64 {
            return Err(invalid("MODEL_MAX_CONCURRENCY", "must be in 1..=64"));
        }
        if self.max_queue > 10_000 {
            return Err(invalid("MODEL_MAX_QUEUE", "must be <= 10000"));
        }
        if self.request_timeout_ms == 0
            || self.stream_timeout_ms == 0
            || self.connect_timeout_ms == 0
            || self.queue_timeout_ms == 0
        {
            return Err(invalid("MODEL_*_TIMEOUT_MS", "timeouts must be >= 1"));
        }
        if self.max_tokens == 0 || self.max_tokens > 8192 {
            return Err(invalid("MODEL_MAX_TOKENS", "must be in 1..=8192"));
        }
        if !(self.temperature.is_finite() && (0.0..=2.0).contains(&self.temperature)) {
            return Err(invalid("MODEL_TEMPERATURE", "must be in 0..=2"));
        }
        if self.canary_percent > 100 {
            return Err(invalid("MODEL_CANARY_PERCENT", "must be in 0..=100"));
        }
        if self.canary_percent > 0 && self.canary_model.as_deref().unwrap_or("").is_empty() {
            return Err(invalid(
                "MODEL_CANARY_MODEL",
                "required when MODEL_CANARY_PERCENT > 0",
            ));
        }
        if self.provider == ProviderKind::OpenAiCompatible
            && !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://"))
        {
            return Err(invalid(
                "MODEL_BASE_URL",
                "must start with http:// or https://",
            ));
        }
        Ok(())
    }

    /// 所有允许客户端显式选择的模型（主模型、灰度、备用）。
    pub fn allowed_models(&self) -> Vec<String> {
        let mut models = vec![self.primary_model.clone()];
        if let Some(canary) = &self.canary_model {
            if !canary.is_empty() && !models.contains(canary) {
                models.push(canary.clone());
            }
        }
        for model in &self.fallback_models {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
        models
    }
}

fn invalid(key: &'static str, message: &str) -> ConfigError {
    ConfigError {
        key,
        message: message.to_string(),
    }
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_dev_friendly() {
        let config = ModelConfig::default();
        assert_eq!(config.provider, ProviderKind::Mock);
        assert!(config.enabled);
        assert_eq!(config.max_concurrency, 4);
        assert_eq!(config.request_timeout_ms, 1500);
    }

    #[test]
    fn validate_rejects_bad_values() {
        let config = ModelConfig {
            max_concurrency: 0,
            ..ModelConfig::default()
        };
        assert!(config.validate().is_err());

        let config = ModelConfig {
            canary_percent: 10,
            ..ModelConfig::default()
        };
        assert!(config.validate().is_err());

        let config = ModelConfig {
            temperature: 3.0,
            ..ModelConfig::default()
        };
        assert!(config.validate().is_err());

        let config = ModelConfig {
            max_tokens: 9000,
            ..ModelConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn allowed_models_deduplicates() {
        let config = ModelConfig {
            fallback_models: vec![
                "Qwen2.5-Coder-7B-Instruct-AWQ".to_string(),
                "Qwen2.5-Coder-14B-Instruct-AWQ".to_string(),
            ],
            canary_model: Some("DeepSeek-Coder-V2-Lite-Instruct".to_string()),
            canary_percent: 10,
            ..ModelConfig::default()
        };
        let models = config.allowed_models();
        assert_eq!(models.len(), 3);
    }

    #[test]
    fn secret_is_masked_in_debug() {
        let secret = SecretString::new("sk-super-secret");
        assert!(!format!("{secret:?}").contains("super"));
    }
}
