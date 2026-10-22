//! 防重放签名接入点（为 REQ-202610-1186 预留）。
//!
//! REQ-202610-1186 将实现完整的 `HMAC-SHA256 + Nonce + ±300s 时间窗 + 服务端去重表`。
//! 本模块先把调用链打通：所有埋点上报在进入 handler 前都会经过 [`ReplayGuard`]，
//! 后续只需替换 [`crate::state::AppState`] 中的实现即可，无需改动业务代码。
//!
//! 默认关闭（`TELEMETRY_REPLAY_ENABLED=false`），避免影响联调；
//! 打开后先校验三个签名头是否存在、时间戳是否在 ±300s 窗口内，
//! 签名本身的校验留给 REQ-202610-1186。

use axum::http::HeaderMap;
use chrono::Utc;

use crate::error::AppError;
use crate::state::AppState;

/// 时间戳请求头（Unix 秒）。
pub const TIMESTAMP_HEADER: &str = "x-timestamp";
/// 随机数请求头。
pub const NONCE_HEADER: &str = "x-nonce";
/// 签名请求头。
pub const SIGNATURE_HEADER: &str = "x-signature";
/// 允许的时钟偏移（秒）。
pub const MAX_CLOCK_SKEW_SECS: i64 = 300;

/// 防重放校验上下文。
#[derive(Debug)]
pub struct ReplayContext<'a> {
    /// 当前鉴权用户。
    pub user_id: &'a str,
    /// HTTP 方法。
    pub method: &'a str,
    /// 请求路径。
    pub path: &'a str,
    /// 请求头。
    pub headers: &'a HeaderMap,
}

/// 防重放校验器接口。
pub trait ReplayGuard: Send + Sync {
    /// 校验请求；失败返回统一错误。
    fn verify(&self, context: &ReplayContext<'_>) -> Result<(), AppError>;
}

/// 当前默认实现：开关关闭时直接放行；开启时校验签名头与时间窗。
#[derive(Debug, Default)]
pub struct PendingReplayGuard {
    enabled: bool,
}

impl PendingReplayGuard {
    /// 按配置构造。
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl ReplayGuard for PendingReplayGuard {
    fn verify(&self, context: &ReplayContext<'_>) -> Result<(), AppError> {
        if !self.enabled {
            return Ok(());
        }

        let timestamp = header_value(context.headers, TIMESTAMP_HEADER)?;
        let _nonce = header_value(context.headers, NONCE_HEADER)?;
        let _signature = header_value(context.headers, SIGNATURE_HEADER)?;

        let timestamp: i64 = timestamp.parse().map_err(|_| {
            AppError::invalid_param("x-timestamp must be a Unix timestamp in seconds")
        })?;
        let now = Utc::now().timestamp();
        if now.abs_diff(timestamp) > MAX_CLOCK_SKEW_SECS as u64 {
            return Err(AppError::invalid_param(
                "x-timestamp is outside the allowed ±300s window",
            ));
        }

        // TODO(REQ-202610-1186): 校验 HMAC-SHA256 签名，并做 nonce 服务端去重。
        Ok(())
    }
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, AppError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::invalid_param(format!("missing replay protection header: {name}")))
}

/// 埋点上报入口的便捷校验函数。
pub fn verify_telemetry_request(
    state: &AppState,
    headers: &HeaderMap,
    user_id: &str,
) -> Result<(), AppError> {
    state.replay_guard.verify(&ReplayContext {
        user_id,
        method: "POST",
        path: "/api/v1/telemetry/event",
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context<'a>(headers: &'a HeaderMap) -> ReplayContext<'a> {
        ReplayContext {
            user_id: "u1",
            method: "POST",
            path: "/api/v1/telemetry/event",
            headers,
        }
    }

    #[test]
    fn disabled_guard_allows_request_without_headers() {
        let guard = PendingReplayGuard::new(false);
        assert!(guard.verify(&context(&HeaderMap::new())).is_ok());
    }

    #[test]
    fn enabled_guard_requires_signature_headers() {
        let guard = PendingReplayGuard::new(true);
        assert!(guard.verify(&context(&HeaderMap::new())).is_err());
    }

    #[test]
    fn enabled_guard_accepts_current_timestamp() {
        let guard = PendingReplayGuard::new(true);
        let mut headers = HeaderMap::new();
        headers.insert(
            TIMESTAMP_HEADER,
            Utc::now().timestamp().to_string().parse().unwrap(),
        );
        headers.insert(NONCE_HEADER, "nonce-1".parse().unwrap());
        headers.insert(SIGNATURE_HEADER, "sig".parse().unwrap());
        assert!(guard.verify(&context(&headers)).is_ok());
    }

    #[test]
    fn enabled_guard_rejects_stale_timestamp() {
        let guard = PendingReplayGuard::new(true);
        let mut headers = HeaderMap::new();
        let stale = Utc::now().timestamp() - MAX_CLOCK_SKEW_SECS - 1;
        headers.insert(TIMESTAMP_HEADER, stale.to_string().parse().unwrap());
        headers.insert(NONCE_HEADER, "nonce-1".parse().unwrap());
        headers.insert(SIGNATURE_HEADER, "sig".parse().unwrap());
        assert!(guard.verify(&context(&headers)).is_err());
    }
}
