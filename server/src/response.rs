//! 统一响应结构：`{ code, msg, data }`（与 PRD §5 对齐）。
//!
//! 约定：
//! - `code = 0` 表示成功，非 0 为业务错误码（见 [`crate::error::ErrorCode`]）；
//! - HTTP 状态码与业务错误码保持可映射关系（见 [`crate::error::ErrorCode::http_status`]）；
//! - 错误场景下 `data` 为 `null`，结构保持不变，便于客户端统一处理。

use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// 统一响应包络。
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T> {
    /// 业务码：0 成功，非 0 见 [`crate::error::ErrorCode`]。
    pub code: i32,
    /// 人类可读的提示信息。
    pub msg: String,
    /// 业务数据，错误时为 `null`。
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    /// 成功响应，携带业务数据。
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            msg: "ok".to_string(),
            data: Some(data),
        }
    }
}

impl ApiResponse<()> {
    /// 成功响应，无业务数据（`data: null`）。
    pub fn ok() -> Self {
        Self {
            code: 0,
            msg: "ok".to_string(),
            data: None,
        }
    }

    /// 错误响应（业务代码请优先通过 [`crate::error::AppError`] 构造，以获得正确的 HTTP 状态码）。
    pub fn error(code: i32, msg: impl Into<String>) -> Self {
        Self {
            code,
            msg: msg.into(),
            data: None,
        }
    }
}

impl<T> IntoResponse for ApiResponse<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_serializes_code_zero_and_data() {
        let value =
            serde_json::to_value(ApiResponse::success(serde_json::json!({ "a": 1 }))).unwrap();
        assert_eq!(value["code"], 0);
        assert_eq!(value["msg"], "ok");
        assert_eq!(value["data"]["a"], 1);
    }

    #[test]
    fn error_serializes_null_data() {
        let value = serde_json::to_value(ApiResponse::<()>::error(40000, "bad")).unwrap();
        assert_eq!(value["code"], 40000);
        assert_eq!(value["msg"], "bad");
        assert_eq!(value["data"], serde_json::Value::Null);
    }

    #[test]
    fn ok_serializes_null_data() {
        let value = serde_json::to_value(ApiResponse::<()>::ok()).unwrap();
        assert_eq!(value["code"], 0);
        assert_eq!(value["data"], serde_json::Value::Null);
    }
}
