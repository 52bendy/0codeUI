//! UI 生成 Prompt 组装与单文件 HTML 输出提取 / 基础校验。
//!
//! 完整的 33 类结构 / 安全校验由 REQ-202610-1173 的 `html_validator` 提供；
//! 本模块只做模型接入层必须的「可渲染、单文件、无外链」门槛，避免把明显
//! 不合格的输出交给下游管线。

use super::error::ModelError;
use super::types::ChatMessage;

/// 单文件 HTML 大小上限（与后续校验器保持一致）。
pub const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;

/// UI 生成系统提示词。
pub const SYSTEM_PROMPT: &str = r#"你是 InTimeUI 的资深前端工程师，负责根据用户需求生成可直接在 WebView 中渲染的单文件 HTML 页面。

必须遵守：
1. 只输出一份完整的 HTML 文档，以 <!DOCTYPE html> 开头、</html> 结尾；不要输出 Markdown 代码围栏、解释或多余文字。
2. 所有 CSS 与 JavaScript 必须内联；禁止 <script src>、<link rel="stylesheet">、@import、外部字体 / 图片 / 接口资源。
3. 使用语义化 HTML（header/nav/main/section/article/footer 等），禁止无意义的 div 堆叠。
4. 视觉采用黑白灰主题，支持 light/dark 切换，切换按钮固定在右上角 Header；使用 CSS 自定义属性管理颜色与间距。
5. 响应式布局，移动端优先；交互元素必须有 hover / focus-visible / active 状态。
6. 动画只允许 transform / opacity / clip-path / filter，并尊重 prefers-reduced-motion。
7. 不嵌入任何密钥、token 或用户隐私数据；所有文本必须做 HTML 转义。
8. 页面结构完整、可直接渲染，不依赖任何构建步骤。"#;

/// 组装模型消息。
pub fn build_messages(user_prompt: &str, scene_code: Option<&str>) -> Vec<ChatMessage> {
    let scene = scene_code
        .map(|code| format!("\n场景编码：{code}"))
        .unwrap_or_default();
    vec![
        ChatMessage::system(SYSTEM_PROMPT),
        ChatMessage::user(format!("用户需求：{user_prompt}{scene}")),
    ]
}

/// 从模型输出中提取 HTML，并做基础单文件校验。
pub fn extract_and_validate(content: &str) -> Result<String, ModelError> {
    let html = extract_html(content);
    validate_single_file(&html)?;
    Ok(html)
}

fn extract_html(content: &str) -> String {
    let trimmed = content.trim();
    let without_fence = strip_code_fence(trimmed);
    let lower = without_fence.to_ascii_lowercase();
    let start = lower.find("<!doctype").unwrap_or(0);
    let html = &without_fence[start..];
    match html.to_ascii_lowercase().rfind("</html>") {
        Some(end) => html[..end + "</html>".len()].trim().to_string(),
        None => html.trim().to_string(),
    }
}

fn strip_code_fence(value: &str) -> &str {
    if !value.starts_with("```") {
        return value;
    }
    let body_start = value
        .find('\n')
        .map(|index| index + 1)
        .unwrap_or(value.len());
    let body = &value[body_start..];
    match body.rfind("```") {
        Some(end) => body[..end].trim(),
        None => body.trim(),
    }
}

fn validate_single_file(html: &str) -> Result<(), ModelError> {
    if html.is_empty() {
        return Err(ModelError::InvalidOutput("empty output".to_string()));
    }
    if html.len() > MAX_HTML_BYTES {
        return Err(ModelError::InvalidOutput(format!(
            "document exceeds {MAX_HTML_BYTES} bytes"
        )));
    }
    let lower = html.to_ascii_lowercase();
    if !lower.starts_with("<!doctype html") {
        return Err(ModelError::InvalidOutput(
            "missing <!DOCTYPE html>".to_string(),
        ));
    }
    for required in ["<html", "</html>", "<head", "</head>", "<body", "</body>"] {
        if !lower.contains(required) {
            return Err(ModelError::InvalidOutput(format!("missing {required}")));
        }
    }
    for forbidden in [
        "<script src",
        "<link",
        "@import",
        "url(http",
        "url(//",
        "<iframe",
        "<object",
        "<embed",
    ] {
        if lower.contains(forbidden) {
            return Err(ModelError::InvalidOutput(format!(
                "external or embedded resource is not allowed: {forbidden}"
            )));
        }
    }
    for attr in ["src=\"http", "src='http", "href=\"http", "href='http"] {
        if lower.contains(attr) {
            return Err(ModelError::InvalidOutput(format!(
                "external URL is not allowed: {attr}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "<!DOCTYPE html><html><head></head><body><h1>ok</h1></body></html>";

    #[test]
    fn extracts_fenced_html() {
        let content = format!("```html\n{VALID}\n```");
        assert_eq!(extract_and_validate(&content).unwrap(), VALID);
    }

    #[test]
    fn extracts_html_after_preamble() {
        let content = format!("好的，以下是页面：\n{VALID}\n希望有帮助。");
        assert_eq!(extract_and_validate(&content).unwrap(), VALID);
    }

    #[test]
    fn rejects_external_script() {
        let content = "<!DOCTYPE html><html><head></head><body><script src=\"https://x\"></script></body></html>";
        assert!(extract_and_validate(content).is_err());
    }

    #[test]
    fn rejects_missing_structure() {
        assert!(extract_and_validate("<div>hi</div>").is_err());
    }

    #[test]
    fn messages_include_scene() {
        let messages = build_messages("订单详情", Some("ORDER_DETAIL_PAGE"));
        assert_eq!(messages.len(), 2);
        assert!(messages[1].content.contains("ORDER_DETAIL_PAGE"));
    }
}
