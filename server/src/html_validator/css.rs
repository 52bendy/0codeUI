//! CSS 安全扫描：外部资源、危险表达式与语法完整性。

use crate::html_validator::config::ValidatorConfig;
use crate::html_validator::model::IssueKind;
use crate::html_validator::sink::IssueSink;
use crate::html_validator::url::{classify_url, host_allowed, UrlKind};

/// 扫描一段 CSS 文本（`<style>` 内容或 `style` 属性值）。
pub(crate) fn scan_css<'a>(
    css: &str,
    base_offset: usize,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'a>,
) {
    let bytes = css.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'') {
            index = skip_css_string(css, index, base_offset, sink);
            continue;
        }

        if bytes[index..].starts_with(b"/*") {
            match css[index + 2..].find("*/") {
                Some(relative) => index += 2 + relative + 2,
                None => {
                    sink.error(IssueKind::InvalidCss, base_offset + index, "CSS 注释未闭合");
                    return;
                }
            }
            continue;
        }

        if starts_keyword(bytes, index, b"@import") {
            sink.error(
                IssueKind::ExternalCssResource,
                base_offset + index,
                "禁止 @import 外部样式",
            );
            index += 7;
            continue;
        }

        if starts_keyword(bytes, index, b"url") {
            let Some((value, next)) = parse_url_function(css, index, base_offset, sink) else {
                return;
            };
            check_css_url(&value, base_offset + index, config, sink);
            index = next;
            continue;
        }

        if starts_keyword(bytes, index, b"expression") {
            sink.error(
                IssueKind::UnsafeCss,
                base_offset + index,
                "禁止 CSS expression()",
            );
            index += 10;
            continue;
        }

        if starts_keyword(bytes, index, b"-moz-binding") {
            sink.error(
                IssueKind::UnsafeCss,
                base_offset + index,
                "禁止 -moz-binding",
            );
            index += 13;
            continue;
        }

        if starts_keyword(bytes, index, b"behavior") && is_property_colon(bytes, index + 8) {
            sink.error(
                IssueKind::UnsafeCss,
                base_offset + index,
                "禁止 behavior 属性",
            );
            index += 8;
            continue;
        }

        if starts_with_ci(bytes, index, b"javascript:") {
            sink.error(
                IssueKind::DisallowedUrlScheme,
                base_offset + index,
                "CSS 中出现 javascript: 协议",
            );
            index += 11;
            continue;
        }

        index += 1;
    }
}

fn skip_css_string(css: &str, start: usize, base_offset: usize, sink: &mut IssueSink<'_>) -> usize {
    let bytes = css.as_bytes();
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return index + 1,
            b'\n' => {
                sink.error(
                    IssueKind::InvalidCss,
                    base_offset + start,
                    "CSS 字符串未闭合",
                );
                return index + 1;
            }
            _ => index += 1,
        }
    }
    sink.error(
        IssueKind::InvalidCss,
        base_offset + start,
        "CSS 字符串未闭合",
    );
    bytes.len()
}

fn starts_with_ci(bytes: &[u8], index: usize, keyword: &[u8]) -> bool {
    bytes.len() >= index + keyword.len()
        && bytes[index..index + keyword.len()].eq_ignore_ascii_case(keyword)
}

fn starts_keyword(bytes: &[u8], index: usize, keyword: &[u8]) -> bool {
    if !starts_with_ci(bytes, index, keyword) {
        return false;
    }
    let end = index + keyword.len();
    let before_ok =
        index == 0 || !(bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'-');
    let after_ok =
        end == bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-');
    before_ok && after_ok
}

fn is_property_colon(bytes: &[u8], mut index: usize) -> bool {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    bytes.get(index) == Some(&b':')
}

/// 解析 `url(...)`，返回 URL 内容与结束后位置。
fn parse_url_function(
    css: &str,
    start: usize,
    base_offset: usize,
    sink: &mut IssueSink<'_>,
) -> Option<(String, usize)> {
    let bytes = css.as_bytes();
    let mut index = start + 3;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index) != Some(&b'(') {
        return Some((String::new(), start + 3));
    }
    index += 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    if index < bytes.len() && matches!(bytes[index], b'"' | b'\'') {
        let quote = bytes[index];
        let value_start = index + 1;
        let mut cursor = value_start;
        while cursor < bytes.len() && bytes[cursor] != quote {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            sink.error(
                IssueKind::InvalidCss,
                base_offset + start,
                "url() 字符串未闭合",
            );
            return None;
        }
        let value = &css[value_start..cursor];
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b')') {
            sink.error(IssueKind::InvalidCss, base_offset + start, "url() 未闭合");
            return None;
        }
        return Some((value.to_string(), cursor + 1));
    }

    let value_start = index;
    while index < bytes.len() && bytes[index] != b')' {
        index += 1;
    }
    if index >= bytes.len() {
        sink.error(IssueKind::InvalidCss, base_offset + start, "url() 未闭合");
        return None;
    }
    let value = css[value_start..index].trim();
    Some((value.to_string(), index + 1))
}

fn check_css_url(raw: &str, offset: usize, config: &ValidatorConfig, sink: &mut IssueSink<'_>) {
    let decoded = decode_css_escapes(raw);
    match classify_url(&decoded) {
        UrlKind::Empty | UrlKind::Fragment => {}
        UrlKind::Data => {
            if !config.allow_data_uris {
                sink.error(IssueKind::ExternalCssResource, offset, "禁止 data: 资源");
            }
        }
        UrlKind::Http(host) | UrlKind::ProtocolRelative(host) => {
            if !host_allowed(&host, &config.allowed_external_hosts) {
                sink.error(
                    IssueKind::ExternalCssResource,
                    offset,
                    format!("CSS 引用了外部资源：{raw}"),
                );
            }
        }
        UrlKind::Relative => sink.error(
            IssueKind::ExternalCssResource,
            offset,
            format!("CSS 引用了单文件之外的资源：{raw}"),
        ),
        UrlKind::Script | UrlKind::VbScript => sink.error(
            IssueKind::DisallowedUrlScheme,
            offset,
            "CSS 中出现危险 URL 协议",
        ),
        UrlKind::Other(_) => sink.error(
            IssueKind::ExternalCssResource,
            offset,
            format!("CSS 引用了不支持的资源协议：{raw}"),
        ),
    }
}

/// 解码 CSS 转义（`\3a ` / `\:` 等），避免绕过协议检查。
fn decode_css_escapes(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            None | Some('\n') => {}
            Some(hex) if hex.is_ascii_hexdigit() => {
                let mut digits = String::from(hex);
                while digits.len() < 6 {
                    match chars.peek() {
                        Some(next) if next.is_ascii_hexdigit() => {
                            digits.push(*next);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if chars.peek().is_some_and(|next| next.is_ascii_whitespace()) {
                    chars.next();
                }
                if let Some(decoded) = u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    output.push(decoded);
                }
            }
            Some(other) => output.push(other),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_validator::source::SourceMap;

    fn scan(css: &str) -> Vec<IssueKind> {
        let map = SourceMap::new(css);
        let mut sink = IssueSink::new(&map, 100);
        scan_css(css, 0, &ValidatorConfig::default(), &mut sink);
        sink.finish().iter().map(|issue| issue.kind).collect()
    }

    #[test]
    fn allows_inline_and_data_urls() {
        assert!(scan("body{background:url(data:image/png;base64,AA)}").is_empty());
        assert!(scan("body{background:url(#gradient)}").is_empty());
    }

    #[test]
    fn rejects_external_and_relative_urls() {
        assert_eq!(
            scan("body{background:url(https://cdn.example.com/a.png)}"),
            vec![IssueKind::ExternalCssResource]
        );
        assert_eq!(
            scan("body{background:url(images/a.png)}"),
            vec![IssueKind::ExternalCssResource]
        );
    }

    #[test]
    fn rejects_import_and_expressions() {
        assert_eq!(
            scan("@import 'https://cdn.example.com/a.css';"),
            vec![IssueKind::ExternalCssResource]
        );
        assert_eq!(
            scan("div{width:expression(alert(1))}"),
            vec![IssueKind::UnsafeCss]
        );
    }

    #[test]
    fn rejects_escaped_javascript_scheme() {
        assert_eq!(
            scan("div{background:url(java\\73 cript:alert(1))}"),
            vec![IssueKind::DisallowedUrlScheme]
        );
    }

    #[test]
    fn reports_unclosed_url() {
        assert_eq!(scan("div{background:url("), vec![IssueKind::InvalidCss]);
    }

    #[test]
    fn ignores_url_inside_comment() {
        assert!(scan("/* url(https://x/a.png) */ body{color:red}").is_empty());
    }
}
