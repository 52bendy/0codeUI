//! 简单鉴权与用户标识（user_id 体系）。
//!
//! - 生产：`Authorization: Bearer <token>`，token → user_id 映射由
//!   `AUTH_STATIC_TOKENS` 环境变量注入（不落库、不入仓）；
//! - 开发（`AUTH_DEV_MODE=true`）：额外接受 `Bearer dev:<user_id>` 便于本地联调；
//! - 后续需求（REQ-202610-1186 防重放签名）将在此基础上扩展 Dynamic Token。

use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::AppError;
use crate::state::AppState;

/// 已认证用户（由 [`require_auth`] 中间件注入请求扩展，供 handler 提取）。
#[derive(Debug, Clone)]
pub struct AuthUser {
    /// 用户标识（user_id 体系）。
    pub user_id: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(AppError::unauthorized)
    }
}

/// 鉴权中间件：解析凭据 → 得到 user_id → 注入 [`AuthUser`]。
///
/// 应用于受保护路由（`route_layer`），失败返回统一 401 响应。
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = extract_bearer_token(&req).ok_or_else(AppError::unauthorized)?;
    let user_id = resolve_user_id(&state, &token).ok_or_else(AppError::unauthorized)?;

    req.extensions_mut().insert(AuthUser { user_id });
    Ok(next.run(req).await)
}

/// 从 `Authorization: Bearer <token>` 中取出 token。
fn extract_bearer_token(req: &Request) -> Option<String> {
    let value = req.headers().get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

/// 解析 token 对应的 user_id。
fn resolve_user_id(state: &AppState, token: &str) -> Option<String> {
    if state.config.auth_dev_mode {
        if let Some(user_id) = token.strip_prefix("dev:") {
            let user_id = user_id.trim();
            if !user_id.is_empty() && user_id.len() <= 64 {
                return Some(user_id.to_string());
            }
        }
    }
    state.config.auth_static_tokens.get(token).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn state_with(dev_mode: bool, tokens: &[(&str, &str)]) -> AppState {
        let config = AppConfig {
            auth_dev_mode: dev_mode,
            auth_static_tokens: tokens
                .iter()
                .map(|(token, user_id)| (token.to_string(), user_id.to_string()))
                .collect(),
            ..AppConfig::default()
        };
        AppState::new(config)
    }

    #[test]
    fn dev_mode_accepts_dev_tokens() {
        let state = state_with(true, &[]);
        assert_eq!(
            resolve_user_id(&state, "dev:1001"),
            Some("1001".to_string())
        );
        assert_eq!(resolve_user_id(&state, "dev:"), None);
        assert_eq!(resolve_user_id(&state, "plain"), None);
    }

    #[test]
    fn static_tokens_resolve_and_dev_tokens_rejected_when_disabled() {
        let state = state_with(false, &[("tok-a", "u42")]);
        assert_eq!(resolve_user_id(&state, "tok-a"), Some("u42".to_string()));
        assert_eq!(resolve_user_id(&state, "nope"), None);
        assert_eq!(resolve_user_id(&state, "dev:1"), None);
    }
}
