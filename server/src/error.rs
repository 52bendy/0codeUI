//! 全局错误码规范与错误类型。
//!
//! 错误码分配规则（与 README「统一响应与错误码」一节保持一致）：
//! - `0`：成功；
//! - `4xxxx`：客户端错误，前三位对齐 HTTP 状态码（400xx / 401xx / 403xx / 404xx / 429xx）；
//! - `5xxxx`：服务端错误（500xx / 503xx / 504xx）；
//! - 后两位为同 HTTP 状态下的细分预留位。

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::response::ApiResponse;

/// 全局错误码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrorCode {
    /// 成功。
    Ok = 0,
    /// 请求参数校验失败（HTTP 400）。
    InvalidParam = 40000,
    /// 未认证 / 凭据无效（HTTP 401）。
    Unauthorized = 40100,
    /// 无权限（HTTP 403）。
    Forbidden = 40300,
    /// 资源不存在（HTTP 404）。
    NotFound = 40400,
    /// 触发限流（HTTP 429）。
    RateLimited = 42900,
    /// 服务内部错误（HTTP 500）。
    Internal = 50000,
    /// 服务暂不可用（HTTP 503）。
    ServiceUnavailable = 50300,
    /// 请求处理超时（HTTP 504）。
    Timeout = 50400,
}

impl ErrorCode {
    /// 数值形式（用于响应体与日志）。
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// 对应的 HTTP 状态码。
    pub const fn http_status(self) -> StatusCode {
        match self {
            Self::Ok => StatusCode::OK,
            Self::InvalidParam => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
        }
    }

    /// 默认提示文案。
    pub const fn default_message(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::InvalidParam => "invalid parameter",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not found",
            Self::RateLimited => "too many requests",
            Self::Internal => "internal server error",
            Self::ServiceUnavailable => "service unavailable",
            Self::Timeout => "request timeout",
        }
    }
}

/// 业务错误类型：可直接作为 handler / 中间件的返回错误，自动转换为统一响应。
#[derive(Debug)]
pub struct AppError {
    /// 业务错误码。
    pub code: ErrorCode,
    /// 返回给客户端的提示信息。
    pub message: String,
    /// 内部诊断信息（仅落日志，不返回给客户端）。
    pub detail: Option<String>,
    /// 限流等场景下建议客户端等待的秒数（`Retry-After`）。
    pub retry_after_secs: Option<u64>,
}

impl AppError {
    /// 构造错误。
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            retry_after_secs: None,
        }
    }

    /// 400：参数校验失败。
    pub fn invalid_param(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParam, message)
    }

    /// 401：未认证 / 凭据无效。
    pub fn unauthorized() -> Self {
        Self::new(
            ErrorCode::Unauthorized,
            ErrorCode::Unauthorized.default_message(),
        )
    }

    /// 403：无权限。
    pub fn forbidden() -> Self {
        Self::new(ErrorCode::Forbidden, ErrorCode::Forbidden.default_message())
    }

    /// 404：资源不存在。
    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound, ErrorCode::NotFound.default_message())
    }

    /// 429：触发限流。
    pub fn rate_limited(retry_after_secs: u64) -> Self {
        let mut error = Self::new(
            ErrorCode::RateLimited,
            ErrorCode::RateLimited.default_message(),
        );
        error.retry_after_secs = Some(retry_after_secs.max(1));
        error
    }

    /// 500：内部错误（detail 仅记录日志）。
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        let mut error = Self::new(ErrorCode::Internal, ErrorCode::Internal.default_message());
        error.detail = Some(detail.to_string());
        error
    }

    /// 504：请求处理超时。
    pub fn timeout() -> Self {
        Self::new(ErrorCode::Timeout, ErrorCode::Timeout.default_message())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_i32(), self.message)
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.code.http_status();

        if let Some(detail) = &self.detail {
            tracing::error!(
                code = self.code.as_i32(),
                detail = %detail,
                "request failed with internal error"
            );
        } else if status.is_server_error() {
            tracing::error!(
                code = self.code.as_i32(),
                message = %self.message,
                "request failed"
            );
        } else {
            tracing::warn!(
                code = self.code.as_i32(),
                message = %self.message,
                "request rejected"
            );
        }

        let body = ApiResponse::<()>::error(self.code.as_i32(), self.message);
        let mut response = (status, Json(body)).into_response();

        if let Some(retry_after) = self.retry_after_secs {
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_map_to_http_status() {
        assert_eq!(ErrorCode::Ok.http_status(), StatusCode::OK);
        assert_eq!(
            ErrorCode::InvalidParam.http_status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ErrorCode::Unauthorized.http_status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(ErrorCode::Forbidden.http_status(), StatusCode::FORBIDDEN);
        assert_eq!(ErrorCode::NotFound.http_status(), StatusCode::NOT_FOUND);
        assert_eq!(
            ErrorCode::RateLimited.http_status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            ErrorCode::Internal.http_status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            ErrorCode::Timeout.http_status(),
            StatusCode::GATEWAY_TIMEOUT
        );
    }

    #[test]
    fn error_code_numeric_values_follow_convention() {
        assert_eq!(ErrorCode::Ok.as_i32(), 0);
        assert_eq!(ErrorCode::InvalidParam.as_i32(), 40000);
        assert_eq!(ErrorCode::Unauthorized.as_i32(), 40100);
        assert_eq!(ErrorCode::Forbidden.as_i32(), 40300);
        assert_eq!(ErrorCode::NotFound.as_i32(), 40400);
        assert_eq!(ErrorCode::RateLimited.as_i32(), 42900);
        assert_eq!(ErrorCode::Internal.as_i32(), 50000);
        assert_eq!(ErrorCode::Timeout.as_i32(), 50400);
    }

    #[test]
    fn rate_limited_sets_retry_after() {
        let error = AppError::rate_limited(0);
        assert_eq!(error.retry_after_secs, Some(1));
    }
}
