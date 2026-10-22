//! UI 生成用例：请求校验 → 场景查找 → 模板渲染 → 响应组装。
//!
//! 本模块不依赖 Axum，路由层只负责鉴权、JSON 提取与统一响应包装，
//! 便于对契约与错误码做纯单元测试。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

use crate::error::{AppError, ErrorCode};
use crate::ui::context::{SceneContext, SceneError};
use crate::ui::device::DeviceInfo;
use crate::ui::scene::{SceneDefinition, REGISTRY};

/// UI 生成请求（PRD §5.1）。
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UiGenerateRequest {
    /// 调用方用户标识。
    #[validate(length(min = 1, max = 64, message = "user_id must be 1..=64 characters"))]
    pub user_id: String,
    /// 场景编码，必须是注册表中的精确值。
    #[validate(length(min = 1, max = 64, message = "scene_code must be 1..=64 characters"))]
    pub scene_code: String,
    /// 设备信息。
    pub device_info: DeviceInfo,
    /// 场景业务数据（必须是 JSON 对象）。
    pub business_data: Value,
}

/// 响应中的渲染类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RenderType {
    /// 单文件 HTML（内联 CSS / JS，无外部依赖）。
    SingleFileHtml,
}

/// UI 生成响应数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiGenerateData {
    /// 渲染类型，第一阶段固定为 `SINGLE_FILE_HTML`。
    pub render_type: RenderType,
    /// 可追溯版本串：`<strategy>:<scene_code>:<template_version>`。
    pub version: String,
    /// 完整单文件 HTML。
    pub html_content: String,
}

/// 执行一次 UI 生成。
pub fn generate_ui(request: &UiGenerateRequest) -> Result<UiGenerateData, AppError> {
    request
        .validate()
        .map_err(|errors| AppError::invalid_param(format!("invalid request: {errors}")))?;
    validate_device_info(&request.device_info)?;

    let platform = request
        .device_info
        .normalized_platform()
        .map_err(AppError::invalid_param)?;

    let scene = REGISTRY
        .get(&request.scene_code)
        .ok_or_else(|| unknown_scene_error(&request.scene_code))?;

    ensure_renderable(scene)?;
    ensure_business_data(&request.business_data, scene)?;

    let version = scene.version();
    let context = SceneContext {
        scene_code: &request.scene_code,
        screen_width: request.device_info.screen_width,
        screen_height: request.device_info.screen_height,
        platform,
        business_data: &request.business_data,
        version: &version,
        template_version: scene.config.template_version,
    };

    let renderer = scene
        .renderer
        .expect("ensure_renderable guarantees a renderer");
    let html_content = renderer(&context).map_err(map_scene_error)?;
    ensure_complete_document(&html_content)?;

    Ok(UiGenerateData {
        render_type: RenderType::SingleFileHtml,
        version,
        html_content,
    })
}

/// 设备字段范围校验（给出字段级错误，不依赖嵌套校验器的抽象错误）。
fn validate_device_info(device: &DeviceInfo) -> Result<(), AppError> {
    if !(1..=20000).contains(&device.screen_width) {
        return Err(AppError::invalid_param(
            "device_info.screen_width must be within 1..=20000",
        ));
    }
    if !(1..=20000).contains(&device.screen_height) {
        return Err(AppError::invalid_param(
            "device_info.screen_height must be within 1..=20000",
        ));
    }
    let platform_len = device.platform.trim().chars().count();
    if !(1..=32).contains(&platform_len) {
        return Err(AppError::invalid_param(
            "device_info.platform must be 1..=32 characters",
        ));
    }
    Ok(())
}

/// 未知场景：明确错误码 40410，并列出可用场景。
fn unknown_scene_error(scene_code: &str) -> AppError {
    let available = REGISTRY.codes().join(", ");
    AppError::new(
        ErrorCode::SceneNotFound,
        format!("unknown scene_code `{scene_code}`; available scenes: {available}"),
    )
}

/// 场景已注册但当前没有渲染实现（例如动态策略尚未接入）。
fn ensure_renderable(scene: &SceneDefinition) -> Result<(), AppError> {
    if scene.is_renderable() {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::ServiceUnavailable,
        format!(
            "scene `{}` is registered with strategy `{}` but no renderer is available",
            scene.code, scene.config.strategy
        ),
    ))
}

/// 校验 `business_data` 是对象且不超过场景配置的字节上限。
fn ensure_business_data(business_data: &Value, scene: &SceneDefinition) -> Result<(), AppError> {
    if !business_data.is_object() {
        return Err(AppError::invalid_param(
            "business_data must be a JSON object",
        ));
    }
    let size = serde_json::to_vec(business_data)
        .map_err(|error| AppError::internal(format!("serialize business_data: {error}")))?
        .len();
    if size > scene.config.max_business_data_bytes {
        return Err(AppError::invalid_param(format!(
            "business_data must be at most {} bytes (got {size})",
            scene.config.max_business_data_bytes
        )));
    }
    Ok(())
}

/// 防御性校验：渲染器必须返回完整的 HTML 文档。
fn ensure_complete_document(html: &str) -> Result<(), AppError> {
    let trimmed = html.trim_end();
    if trimmed.starts_with("<!DOCTYPE html>") && trimmed.ends_with("</html>") {
        return Ok(());
    }
    Err(AppError::internal(
        "renderer returned an incomplete HTML document",
    ))
}

/// 场景错误 → 统一错误码。
fn map_scene_error(error: SceneError) -> AppError {
    match error {
        SceneError::InvalidBusinessData(message) => AppError::invalid_param(message),
        SceneError::Render(message) => AppError::internal(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(scene_code: &str, business_data: Value) -> UiGenerateRequest {
        UiGenerateRequest {
            user_id: "usr_9527".to_string(),
            scene_code: scene_code.to_string(),
            device_info: DeviceInfo {
                screen_width: 390,
                screen_height: 844,
                platform: "Android".to_string(),
            },
            business_data,
        }
    }

    fn order_detail_data() -> Value {
        json!({
            "order_id": "ORD_20261014_001",
            "status": "PAID",
            "items": [
                { "name": "智能降噪耳机", "price": 299.00, "qty": 1 }
            ]
        })
    }

    #[test]
    fn generates_order_detail_single_file_html() {
        let data = generate_ui(&request("ORDER_DETAIL_PAGE", order_detail_data())).unwrap();
        assert_eq!(data.render_type, RenderType::SingleFileHtml);
        assert_eq!(data.version, "template:ORDER_DETAIL_PAGE:v1");
        assert!(data.html_content.starts_with("<!DOCTYPE html>"));
        assert!(data.html_content.contains("智能降噪耳机"));
        assert!(data.html_content.contains("</html>"));
    }

    #[test]
    fn render_type_serializes_as_contract_value() {
        let value = serde_json::to_value(RenderType::SingleFileHtml).unwrap();
        assert_eq!(value, json!("SINGLE_FILE_HTML"));
    }

    #[test]
    fn unknown_scene_returns_40410_and_lists_available_scenes() {
        let error = generate_ui(&request("NOT_A_SCENE", json!({}))).unwrap_err();
        assert_eq!(error.code, ErrorCode::SceneNotFound);
        assert_eq!(error.code.as_i32(), 40410);
        assert!(error.message.contains("ORDER_DETAIL_PAGE"));
        assert!(error.message.contains("ORDER_LIST_PAGE"));
        assert!(error.message.contains("PRODUCT_DETAIL_PAGE"));
    }

    #[test]
    fn invalid_business_data_maps_to_invalid_param() {
        let data = json!({
            "order_id": "ORD_1",
            "status": "PAID",
            "items": [{ "name": "item", "price": -1.00, "qty": 1 }]
        });
        let error = generate_ui(&request("ORDER_DETAIL_PAGE", data)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParam);
        assert_eq!(error.code.as_i32(), 40000);
    }

    #[test]
    fn business_data_must_be_object() {
        let error = generate_ui(&request("ORDER_DETAIL_PAGE", json!([1, 2, 3]))).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParam);
        assert!(error.message.contains("JSON object"));
    }

    #[test]
    fn oversized_business_data_is_rejected() {
        let data = json!({
            "order_id": "ORD_1",
            "status": "PAID",
            "items": [{ "name": "x".repeat(65 * 1024), "price": 1.00, "qty": 1 }]
        });
        let error = generate_ui(&request("ORDER_DETAIL_PAGE", data)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParam);
        assert!(error.message.contains("at most"));
    }

    #[test]
    fn invalid_device_platform_is_rejected() {
        let mut req = request("ORDER_DETAIL_PAGE", order_detail_data());
        req.device_info.platform = "blackberry".to_string();
        let error = generate_ui(&req).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParam);
        assert!(error.message.contains("android, ios, web, harmonyos"));
    }

    #[test]
    fn version_tracks_strategy_scene_and_template_version() {
        let data = generate_ui(&request(
            "PRODUCT_DETAIL_PAGE",
            json!({
                "product_id": "PRD_1",
                "name": "商品",
                "price": 99.00
            }),
        ))
        .unwrap();
        assert_eq!(data.version, "template:PRODUCT_DETAIL_PAGE:v1");
    }
}
