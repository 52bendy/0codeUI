//! 模型接入错误类型。

use std::fmt;

/// 模型调用错误。
#[derive(Debug, Clone)]
pub enum ModelError {
    /// 模型功能未启用。
    Disabled,
    /// 请求参数非法（客户端错误）。
    InvalidRequest(String),
    /// 客户端指定了未注册的模型。
    UnknownModel(String),
    /// 排队已满。
    QueueFull,
    /// 排队等待超时。
    QueueTimeout,
    /// 上游推理超时。
    Timeout,
    /// 上游返回错误（状态码 + 摘要）。
    Upstream {
        /// HTTP 状态码；网络错误时为 0。
        status: u16,
        /// 可读摘要（已截断，不含密钥）。
        message: String,
        /// 是否可重试。
        retryable: bool,
    },
    /// 上游返回内容无法解析。
    Protocol(String),
    /// 模型输出不是合法的单文件 HTML。
    InvalidOutput(String),
}

impl ModelError {
    /// 是否可重试。
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Timeout => true,
            Self::Upstream { retryable, .. } => *retryable,
            _ => false,
        }
    }

    /// 上游 429 时建议的等待秒数（若响应头提供）。
    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::Upstream { status: 429, .. } => Some(1),
            _ => None,
        }
    }

    /// 错误码（写入日志与响应）。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Disabled => "model_disabled",
            Self::InvalidRequest(_) => "invalid_request",
            Self::UnknownModel(_) => "unknown_model",
            Self::QueueFull => "queue_full",
            Self::QueueTimeout => "queue_timeout",
            Self::Timeout => "timeout",
            Self::Upstream { .. } => "upstream_error",
            Self::Protocol(_) => "protocol_error",
            Self::InvalidOutput(_) => "invalid_output",
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => f.write_str("model service is disabled"),
            Self::InvalidRequest(message) => write!(f, "invalid request: {message}"),
            Self::UnknownModel(model) => write!(f, "unknown model: {model}"),
            Self::QueueFull => f.write_str("model queue is full"),
            Self::QueueTimeout => f.write_str("model queue wait timed out"),
            Self::Timeout => f.write_str("model inference timed out"),
            Self::Upstream {
                status, message, ..
            } => write!(f, "upstream error ({status}): {message}"),
            Self::Protocol(message) => write!(f, "invalid upstream response: {message}"),
            Self::InvalidOutput(message) => write!(f, "model output rejected: {message}"),
        }
    }
}

impl std::error::Error for ModelError {}
