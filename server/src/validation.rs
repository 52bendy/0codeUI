//! 请求参数校验：`ValidatedJson` 提取器（反序列化 + `validator` 规则校验）。
//!
//! Axum 生态中，请求体校验的惯用做法是「带校验的提取器」：
//! handler 声明 `ValidatedJson<T>` 即自动完成 JSON 解析与字段规则校验，
//! 失败时返回统一的 400 错误响应（code = 40000）。

use axum::extract::{FromRequest, Request};
use axum::Json;
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::AppError;

/// 带校验的 JSON 提取器。
///
/// 用法：
/// ```ignore
/// async fn handler(ValidatedJson(payload): ValidatedJson<MyRequest>) { ... }
/// ```
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|rejection| {
                AppError::invalid_param(format!("invalid request body: {}", rejection.body_text()))
            })?;

        value
            .validate()
            .map_err(|errors| AppError::invalid_param(format_validation_errors(&errors)))?;

        Ok(Self(value))
    }
}

/// 将校验错误折叠为可读文案，例如：`message: message must be 1..=500 characters`。
fn format_validation_errors(errors: &validator::ValidationErrors) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (field, field_errors) in errors.field_errors() {
        for error in field_errors {
            let message = error
                .message
                .as_ref()
                .map(|m| m.to_string())
                .unwrap_or_else(|| error.code.to_string());
            parts.push(format!("{field}: {message}"));
        }
    }
    if parts.is_empty() {
        "validation failed".to_string()
    } else {
        parts.join("; ")
    }
}
