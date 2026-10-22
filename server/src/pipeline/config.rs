//! 生成管线配置（`PIPELINE_*` 环境变量）。
//!
//! 时间预算以「首屏感知 ≤ 1.5s」为硬约束：`PIPELINE_TIMEOUT_MS` 是端到端总预算，
//! `PIPELINE_FALLBACK_RESERVE_MS` 预留给静态模板兜底，保证模型超时后仍能在预算内
//! 返回一份可渲染页面，而不是白屏或 504。

use std::fmt;
use std::str::FromStr;

use crate::config::{env_parse, optional_env, parse_bool, ConfigError};

/// 输出混淆强度（与 REQ-202610-1184 的强度枚举对齐）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObfuscationMode {
    /// 关闭。
    Off,
    /// 基础：类名 / 属性顺序。
    Basic,
    /// 标准：基础 + JS 标识符 + 包装节点。
    #[default]
    Standard,
}

impl ObfuscationMode {
    /// 配置值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Basic => "basic",
            Self::Standard => "standard",
        }
    }

    /// 是否启用。
    pub const fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

impl fmt::Display for ObfuscationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ObfuscationMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Ok(Self::Off),
            "basic" | "light" => Ok(Self::Basic),
            "standard" | "default" => Ok(Self::Standard),
            other => Err(format!("expected one of off/basic/standard, got `{other}`")),
        }
    }
}

/// 生成管线配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineConfig {
    /// 是否启用动态生成管线：`PIPELINE_ENABLED`（默认 true）。
    pub enabled: bool,
    /// 端到端时间预算（毫秒）：`PIPELINE_TIMEOUT_MS`（默认 1500）。
    pub timeout_ms: u64,
    /// 预留给静态模板兜底的时间（毫秒）：`PIPELINE_FALLBACK_RESERVE_MS`（默认 50）。
    pub fallback_reserve_ms: u64,
    /// 是否启用响应缓存：`PIPELINE_CACHE_ENABLED`（默认 true）。
    pub cache_enabled: bool,
    /// 缓存 TTL（秒）：`PIPELINE_CACHE_TTL_SECS`（默认 300）。
    pub cache_ttl_secs: u64,
    /// 缓存最大条目数：`PIPELINE_CACHE_MAX_ENTRIES`（默认 256）。
    pub cache_max_entries: usize,
    /// 输出混淆强度：`PIPELINE_OBFUSCATION_LEVEL`（off / basic / standard，默认 standard）。
    pub obfuscation_mode: ObfuscationMode,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_ms: 1500,
            fallback_reserve_ms: 50,
            cache_enabled: true,
            cache_ttl_secs: 300,
            cache_max_entries: 256,
            obfuscation_mode: ObfuscationMode::Standard,
        }
    }
}

impl PipelineConfig {
    /// 从环境变量加载并校验。
    pub fn from_env() -> Result<Self, ConfigError> {
        let enabled = match optional_env("PIPELINE_ENABLED") {
            Some(value) => parse_bool("PIPELINE_ENABLED", &value)?,
            None => true,
        };
        let timeout_ms = env_parse("PIPELINE_TIMEOUT_MS", 1500u64)?;
        if !(100..=30_000).contains(&timeout_ms) {
            return Err(ConfigError {
                key: "PIPELINE_TIMEOUT_MS",
                message: "must be in 100..=30000".to_string(),
            });
        }
        let fallback_reserve_ms = env_parse("PIPELINE_FALLBACK_RESERVE_MS", 50u64)?;
        if fallback_reserve_ms >= timeout_ms {
            return Err(ConfigError {
                key: "PIPELINE_FALLBACK_RESERVE_MS",
                message: "must be smaller than PIPELINE_TIMEOUT_MS".to_string(),
            });
        }
        let cache_enabled = match optional_env("PIPELINE_CACHE_ENABLED") {
            Some(value) => parse_bool("PIPELINE_CACHE_ENABLED", &value)?,
            None => true,
        };
        let cache_ttl_secs = env_parse("PIPELINE_CACHE_TTL_SECS", 300u64)?;
        if !(1..=86_400).contains(&cache_ttl_secs) {
            return Err(ConfigError {
                key: "PIPELINE_CACHE_TTL_SECS",
                message: "must be in 1..=86400".to_string(),
            });
        }
        let cache_max_entries = env_parse("PIPELINE_CACHE_MAX_ENTRIES", 256usize)?;
        if !(1..=10_000).contains(&cache_max_entries) {
            return Err(ConfigError {
                key: "PIPELINE_CACHE_MAX_ENTRIES",
                message: "must be in 1..=10000".to_string(),
            });
        }
        let obfuscation_mode = match optional_env("PIPELINE_OBFUSCATION_LEVEL") {
            Some(value) => ObfuscationMode::from_str(&value).map_err(|message| ConfigError {
                key: "PIPELINE_OBFUSCATION_LEVEL",
                message,
            })?,
            None => ObfuscationMode::Standard,
        };

        Ok(Self {
            enabled,
            timeout_ms,
            fallback_reserve_ms,
            cache_enabled,
            cache_ttl_secs,
            cache_max_entries,
            obfuscation_mode,
        })
    }

    /// 动态阶段可用预算（总预算减去兜底预留）。
    pub const fn dynamic_budget_ms(&self) -> u64 {
        self.timeout_ms.saturating_sub(self.fallback_reserve_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_first_screen_budget() {
        let config = PipelineConfig::default();
        assert_eq!(config.timeout_ms, 1500);
        assert_eq!(config.dynamic_budget_ms(), 1450);
        assert!(config.enabled);
        assert!(config.cache_enabled);
    }

    #[test]
    fn obfuscation_mode_parsing_is_case_insensitive() {
        assert_eq!(
            ObfuscationMode::from_str("STANDARD").unwrap(),
            ObfuscationMode::Standard
        );
        assert_eq!(
            ObfuscationMode::from_str("off").unwrap(),
            ObfuscationMode::Off
        );
        assert!(ObfuscationMode::from_str("extreme").is_err());
    }
}
