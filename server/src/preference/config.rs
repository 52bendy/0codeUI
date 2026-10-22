//! 偏好数据管道配置（`PREFERENCE_*`）。

use crate::config::{env_parse, optional_env, ConfigError, Environment};

/// 偏好数据管道配置。
#[derive(Debug, Clone)]
pub struct PreferenceConfig {
    /// 管理端令牌（`PREFERENCE_ADMIN_TOKEN`）；配置后管理接口需携带
    /// `X-Preference-Token`。生产环境必填。
    pub admin_token: Option<String>,
    /// 入库最低质量分（`PREFERENCE_MIN_QUALITY`，默认 0.5）。
    pub min_quality: f64,
    /// 自动通过质量分（`PREFERENCE_AUTO_ACCEPT_QUALITY`，默认 0.8）。
    pub auto_accept_quality: f64,
    /// prompt 最大字符数（`PREFERENCE_MAX_PROMPT_CHARS`，默认 8000）。
    pub max_prompt_chars: usize,
    /// 单段 HTML 最大字节数（`PREFERENCE_MAX_HTML_BYTES`，默认 256 KiB）。
    pub max_html_bytes: usize,
    /// 元数据 JSON 最大字节数（`PREFERENCE_MAX_METADATA_BYTES`，默认 8 KiB）。
    pub max_metadata_bytes: usize,
    /// 单次导出最大记录数（`PREFERENCE_EXPORT_MAX_RECORDS`，默认 5000）。
    pub export_max_records: usize,
    /// 导出时单用户最大记录数（`PREFERENCE_EXPORT_MAX_PER_USER`，默认 50）。
    pub export_max_per_user: usize,
    /// 导出时单场景最大记录数（`PREFERENCE_EXPORT_MAX_PER_SCENE`，默认 1000）。
    pub export_max_per_scene: usize,
    /// 导出时用户标识哈希盐（`PREFERENCE_USER_HASH_SALT`）；生产环境必填。
    pub user_hash_salt: String,
}

impl PreferenceConfig {
    /// 从环境变量加载；`env` 用于生产环境必填项校验。
    pub fn from_env(env: Environment) -> Result<Self, ConfigError> {
        let admin_token = optional_env("PREFERENCE_ADMIN_TOKEN");
        let min_quality = env_parse("PREFERENCE_MIN_QUALITY", 0.5f64)?;
        if !(min_quality.is_finite() && (0.0..=1.0).contains(&min_quality)) {
            return Err(ConfigError {
                key: "PREFERENCE_MIN_QUALITY",
                message: "must be in [0, 1]".to_string(),
            });
        }
        let auto_accept_quality = env_parse("PREFERENCE_AUTO_ACCEPT_QUALITY", 0.8f64)?;
        if !(auto_accept_quality.is_finite()
            && auto_accept_quality >= min_quality
            && auto_accept_quality <= 1.0)
        {
            return Err(ConfigError {
                key: "PREFERENCE_AUTO_ACCEPT_QUALITY",
                message: "must be in [PREFERENCE_MIN_QUALITY, 1]".to_string(),
            });
        }

        let max_prompt_chars = env_parse("PREFERENCE_MAX_PROMPT_CHARS", 8_000usize)?;
        if !(100..=100_000).contains(&max_prompt_chars) {
            return Err(ConfigError {
                key: "PREFERENCE_MAX_PROMPT_CHARS",
                message: "must be in 100..=100000".to_string(),
            });
        }
        let max_html_bytes = env_parse("PREFERENCE_MAX_HTML_BYTES", 262_144usize)?;
        if !(1_024..=2_097_152).contains(&max_html_bytes) {
            return Err(ConfigError {
                key: "PREFERENCE_MAX_HTML_BYTES",
                message: "must be in 1024..=2097152".to_string(),
            });
        }
        let max_metadata_bytes = env_parse("PREFERENCE_MAX_METADATA_BYTES", 8_192usize)?;
        if !(256..=65_536).contains(&max_metadata_bytes) {
            return Err(ConfigError {
                key: "PREFERENCE_MAX_METADATA_BYTES",
                message: "must be in 256..=65536".to_string(),
            });
        }
        let export_max_records = env_parse("PREFERENCE_EXPORT_MAX_RECORDS", 5_000usize)?;
        if !(1..=50_000).contains(&export_max_records) {
            return Err(ConfigError {
                key: "PREFERENCE_EXPORT_MAX_RECORDS",
                message: "must be in 1..=50000".to_string(),
            });
        }
        let export_max_per_user = env_parse("PREFERENCE_EXPORT_MAX_PER_USER", 50usize)?;
        if !(1..=10_000).contains(&export_max_per_user) {
            return Err(ConfigError {
                key: "PREFERENCE_EXPORT_MAX_PER_USER",
                message: "must be in 1..=10000".to_string(),
            });
        }
        let export_max_per_scene = env_parse("PREFERENCE_EXPORT_MAX_PER_SCENE", 1_000usize)?;
        if !(1..=100_000).contains(&export_max_per_scene) {
            return Err(ConfigError {
                key: "PREFERENCE_EXPORT_MAX_PER_SCENE",
                message: "must be in 1..=100000".to_string(),
            });
        }

        let user_hash_salt = optional_env("PREFERENCE_USER_HASH_SALT")
            .unwrap_or_else(|| "intimeui-dev-preference-salt".to_string());

        if env == Environment::Prod {
            if admin_token.as_deref().unwrap_or("").len() < 16 {
                return Err(ConfigError {
                    key: "PREFERENCE_ADMIN_TOKEN",
                    message: "must be set (>= 16 characters) in prod".to_string(),
                });
            }
            if user_hash_salt.len() < 16 {
                return Err(ConfigError {
                    key: "PREFERENCE_USER_HASH_SALT",
                    message: "must be set (>= 16 characters) in prod".to_string(),
                });
            }
        }

        Ok(Self {
            admin_token,
            min_quality,
            auto_accept_quality,
            max_prompt_chars,
            max_html_bytes,
            max_metadata_bytes,
            export_max_records,
            export_max_per_user,
            export_max_per_scene,
            user_hash_salt,
        })
    }
}

impl Default for PreferenceConfig {
    fn default() -> Self {
        Self {
            admin_token: None,
            min_quality: 0.5,
            auto_accept_quality: 0.8,
            max_prompt_chars: 8_000,
            max_html_bytes: 262_144,
            max_metadata_bytes: 8_192,
            export_max_records: 5_000,
            export_max_per_user: 50,
            export_max_per_scene: 1_000,
            user_hash_salt: "intimeui-dev-preference-salt".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_dev_friendly() {
        let config = PreferenceConfig::default();
        assert!(config.admin_token.is_none());
        assert_eq!(config.min_quality, 0.5);
        assert_eq!(config.auto_accept_quality, 0.8);
        assert!(config.export_max_records >= 1);
    }
}
