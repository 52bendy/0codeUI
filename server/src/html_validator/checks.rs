//! 结构与安全校验主流程。

use crate::html_validator::config::ValidatorConfig;
use crate::html_validator::css::scan_css;
use crate::html_validator::model::{IssueKind, Severity, ValidationResult, ValidationStats};
use crate::html_validator::sink::IssueSink;
use crate::html_validator::source::SourceMap;
use crate::html_validator::tokenizer::{
    is_void_element, tokenize, Attribute, Tag, Token, TokenKind,
};
use crate::html_validator::url::{classify_url, host_allowed, normalize_url, UrlKind};

/// 打开的元素。
struct OpenElement {
    name: String,
    start: usize,
}

/// 文档结构状态。
#[derive(Default)]
struct StructureState {
    first_significant: bool,
    seen_doctype: bool,
    seen_html: bool,
    seen_head: bool,
    seen_body: bool,
    root_closed: bool,
    depth_reported: bool,
}

/// 执行完整校验。
pub(crate) fn validate(html: &str, config: &ValidatorConfig) -> ValidationResult {
    let mut stats = ValidationStats {
        bytes: html.len(),
        ..ValidationStats::default()
    };

    if html.len() > config.max_bytes {
        return too_large(html, config, stats);
    }
    if !has_visible_text(html) {
        let map = SourceMap::new(html);
        let mut sink = IssueSink::new(&map, config.max_issues);
        sink.error(IssueKind::EmptyDocument, 0, "文档为空");
        return finish(sink, stats);
    }

    let map = SourceMap::new(html);
    let mut sink = IssueSink::new(&map, config.max_issues);
    let tokens = tokenize(html, &mut sink);
    let mut state = StructureState::default();
    let mut stack: Vec<OpenElement> = Vec::new();

    for token in &tokens {
        process_token(token, &mut state, &mut stack, config, &mut sink, &mut stats);
    }
    finish_structure(&state, &stack, config, &mut sink);
    finish(sink, stats)
}

fn too_large(html: &str, config: &ValidatorConfig, stats: ValidationStats) -> ValidationResult {
    let map = SourceMap::new(html);
    let mut sink = IssueSink::new(&map, config.max_issues);
    sink.error(
        IssueKind::TooLarge,
        0,
        format!(
            "文档大小 {} 字节超过上限 {} 字节",
            html.len(),
            config.max_bytes
        ),
    );
    finish(sink, stats)
}

fn finish(sink: IssueSink<'_>, stats: ValidationStats) -> ValidationResult {
    let issues = sink.finish();
    let valid = !issues.iter().any(|issue| issue.severity == Severity::Error);
    ValidationResult {
        valid,
        issues,
        stats,
    }
}

fn process_token(
    token: &Token,
    state: &mut StructureState,
    stack: &mut Vec<OpenElement>,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
    stats: &mut ValidationStats,
) {
    match &token.kind {
        TokenKind::Doctype { name } => process_doctype(name, token.start, state, sink),
        TokenKind::Comment => {}
        TokenKind::Text { text } => {
            if has_visible_text(text) {
                state.first_significant = true;
                check_text(token.start, state, stack, sink);
            }
        }
        TokenKind::StartTag(tag) => {
            state.first_significant = true;
            process_start_tag(tag, state, stack, config, sink, stats);
        }
        TokenKind::RawText {
            tag,
            content,
            content_start,
            ..
        } => {
            if tag == "style" {
                scan_css(content, *content_start, config, sink);
            }
        }
        TokenKind::EndTag { name, start } => {
            state.first_significant = true;
            process_end_tag(name, *start, state, stack, sink);
        }
    }
}

fn process_doctype(name: &str, start: usize, state: &mut StructureState, sink: &mut IssueSink<'_>) {
    if state.first_significant {
        sink.error(
            IssueKind::DoctypeNotFirst,
            start,
            "DOCTYPE 必须位于文档最前",
        );
    }
    if name != "html" {
        sink.error(IssueKind::InvalidDoctype, start, "DOCTYPE 必须为 html");
    }
    state.first_significant = true;
    state.seen_doctype = true;
}

fn has_visible_text(text: &str) -> bool {
    text.chars()
        .any(|ch| !ch.is_whitespace() && ch != '\u{feff}')
}

fn check_text(
    start: usize,
    state: &StructureState,
    stack: &[OpenElement],
    sink: &mut IssueSink<'_>,
) {
    if state.root_closed {
        sink.error(IssueKind::ContentOutsideRoot, start, "根元素之外出现文本");
        return;
    }
    let Some(parent) = stack.last() else {
        if !state.seen_html || state.seen_body {
            sink.error(IssueKind::ContentOutsideRoot, start, "根元素之外出现文本");
        } else {
            sink.error(
                IssueKind::MisplacedContent,
                start,
                "html 下不允许直接出现文本",
            );
        }
        return;
    };
    if matches!(parent.name.as_str(), "html" | "head") {
        sink.error(
            IssueKind::MisplacedContent,
            start,
            format!("<{}> 下不允许直接出现文本", parent.name),
        );
    }
}

fn process_start_tag(
    tag: &Tag,
    state: &mut StructureState,
    stack: &mut Vec<OpenElement>,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
    stats: &mut ValidationStats,
) {
    stats.elements += 1;
    match tag.name.as_str() {
        "script" => stats.scripts += 1,
        "style" => stats.styles += 1,
        _ => {}
    }

    if config.is_element_forbidden(&tag.name) {
        sink.error(
            IssueKind::ForbiddenElement,
            tag.start,
            format!("禁止使用 <{}> 元素", tag.name),
        );
    }

    check_attributes(tag, config, sink);
    check_structure(tag, state, stack, sink);

    let depth = stack.len() + 1;
    stats.max_depth = stats.max_depth.max(depth);
    if stack.len() >= config.max_depth && !state.depth_reported {
        state.depth_reported = true;
        sink.error(
            IssueKind::MaxDepthExceeded,
            tag.start,
            format!("嵌套深度超过上限 {}", config.max_depth),
        );
    }

    if is_void_element(&tag.name) {
        return;
    }
    let in_foreign = stack
        .iter()
        .any(|element| matches!(element.name.as_str(), "svg" | "math"));
    if tag.self_closing {
        if !in_foreign {
            sink.error(
                IssueKind::SelfClosingNonVoid,
                tag.start,
                format!("非空元素 <{}> 不允许自闭合", tag.name),
            );
        }
        return;
    }
    stack.push(OpenElement {
        name: tag.name.clone(),
        start: tag.start,
    });
}

fn check_structure(
    tag: &Tag,
    state: &mut StructureState,
    stack: &[OpenElement],
    sink: &mut IssueSink<'_>,
) {
    if state.root_closed {
        sink.error(
            IssueKind::ContentOutsideRoot,
            tag.start,
            "根元素 </html> 之后出现元素",
        );
        return;
    }
    let parent = stack.last().map(|element| element.name.as_str());
    match tag.name.as_str() {
        "html" => {
            if state.seen_html {
                sink.error(
                    IssueKind::DuplicateStructureElement,
                    tag.start,
                    "重复的 <html>",
                );
            } else if !stack.is_empty() || state.root_closed {
                sink.error(
                    IssueKind::MisplacedStructureElement,
                    tag.start,
                    "<html> 必须是根元素",
                );
            } else {
                state.seen_html = true;
            }
        }
        "head" => check_head(tag, state, parent, sink),
        "body" => check_body(tag, state, parent, sink),
        _ => {
            if parent == Some("html") {
                sink.error(
                    IssueKind::MisplacedStructureElement,
                    tag.start,
                    "<html> 下只允许 <head> 与 <body>",
                );
            }
            if stack.is_empty() && !state.seen_html {
                sink.error(
                    IssueKind::ContentOutsideRoot,
                    tag.start,
                    "根元素 <html> 之前出现元素",
                );
            }
        }
    }
}

fn check_head(
    tag: &Tag,
    state: &mut StructureState,
    parent: Option<&str>,
    sink: &mut IssueSink<'_>,
) {
    if !state.seen_html {
        sink.error(
            IssueKind::MisplacedStructureElement,
            tag.start,
            "<head> 必须位于 <html> 内",
        );
    } else if state.seen_head {
        sink.error(
            IssueKind::DuplicateStructureElement,
            tag.start,
            "重复的 <head>",
        );
    } else if state.seen_body {
        sink.error(
            IssueKind::MisplacedStructureElement,
            tag.start,
            "<head> 必须位于 <body> 之前",
        );
    } else if parent != Some("html") {
        sink.error(
            IssueKind::MisplacedStructureElement,
            tag.start,
            "<head> 必须是 <html> 的直接子元素",
        );
    } else {
        state.seen_head = true;
    }
}

fn check_body(
    tag: &Tag,
    state: &mut StructureState,
    parent: Option<&str>,
    sink: &mut IssueSink<'_>,
) {
    if !state.seen_html {
        sink.error(
            IssueKind::MisplacedStructureElement,
            tag.start,
            "<body> 必须位于 <html> 内",
        );
    } else if state.seen_body {
        sink.error(
            IssueKind::DuplicateStructureElement,
            tag.start,
            "重复的 <body>",
        );
    } else if parent != Some("html") {
        sink.error(
            IssueKind::MisplacedStructureElement,
            tag.start,
            "<body> 必须是 <html> 的直接子元素",
        );
    } else {
        if !state.seen_head {
            sink.error(
                IssueKind::MissingStructureElement,
                tag.start,
                "<body> 之前缺少 <head>",
            );
        }
        state.seen_body = true;
    }
}

fn process_end_tag(
    name: &str,
    start: usize,
    state: &mut StructureState,
    stack: &mut Vec<OpenElement>,
    sink: &mut IssueSink<'_>,
) {
    let Some(top) = stack.last() else {
        sink.error(
            IssueKind::UnexpectedClosingTag,
            start,
            format!("多余的结束标签 </{name}>"),
        );
        return;
    };
    if top.name == name {
        stack.pop();
        if name == "html" {
            state.root_closed = true;
        }
        return;
    }

    let Some(index) = stack.iter().rposition(|element| element.name == name) else {
        sink.error(
            IssueKind::UnexpectedClosingTag,
            start,
            format!("结束标签 </{name}> 没有对应的开始标签"),
        );
        return;
    };

    for element in stack.split_off(index + 1).into_iter().rev() {
        sink.error(
            IssueKind::UnclosedTag,
            element.start,
            format!("标签 <{}> 未闭合（遇到 </{name}>）", element.name),
        );
    }
    stack.pop();
    sink.error(
        IssueKind::MismatchedClosingTag,
        start,
        format!("结束标签 </{name}> 与当前打开的标签不匹配"),
    );
    if name == "html" {
        state.root_closed = true;
    }
}

fn finish_structure(
    state: &StructureState,
    stack: &[OpenElement],
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
) {
    for element in stack.iter().rev() {
        sink.error(
            IssueKind::UnclosedTag,
            element.start,
            format!("标签 <{}> 未闭合（文档结束）", element.name),
        );
    }
    if config.require_doctype && !state.seen_doctype {
        sink.error(IssueKind::MissingDoctype, 0, "缺少 <!DOCTYPE html>");
    }
    if !state.seen_html {
        sink.error(IssueKind::MissingStructureElement, 0, "缺少 <html>");
    }
    if !state.seen_head {
        sink.error(IssueKind::MissingStructureElement, 0, "缺少 <head>");
    }
    if !state.seen_body {
        sink.error(IssueKind::MissingStructureElement, 0, "缺少 <body>");
    }
}

fn check_attributes(tag: &Tag, config: &ValidatorConfig, sink: &mut IssueSink<'_>) {
    let rel_is_stylesheet = tag
        .attributes
        .iter()
        .find(|attribute| attribute.name == "rel")
        .and_then(|attribute| attribute.value.as_deref())
        .is_some_and(|value| {
            value
                .split_whitespace()
                .any(|part| part.eq_ignore_ascii_case("stylesheet"))
        });

    for attribute in &tag.attributes {
        check_event_attribute(attribute, config, sink);
        if attribute.name == "style" {
            if let Some(value) = &attribute.value {
                scan_css(value, attribute.value_start, config, sink);
            }
        }
        check_meta_refresh(tag, attribute, sink);
        check_url_attribute(tag, attribute, rel_is_stylesheet, config, sink);
    }
}

fn check_event_attribute(
    attribute: &Attribute,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
) {
    if attribute.name.starts_with("on")
        && attribute.name.len() > 2
        && !config.is_event_allowed(&attribute.name)
    {
        sink.error(
            IssueKind::EventHandlerNotAllowed,
            attribute.name_start,
            format!("事件属性 '{}' 不在白名单内", attribute.name),
        );
    }
}

fn check_meta_refresh(tag: &Tag, attribute: &Attribute, sink: &mut IssueSink<'_>) {
    if tag.name != "meta" || attribute.name != "http-equiv" {
        return;
    }
    let is_refresh = attribute
        .value
        .as_deref()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("refresh"));
    if is_refresh {
        sink.error(
            IssueKind::MetaRefresh,
            attribute.value_start,
            "禁止 meta refresh 跳转",
        );
    }
}

fn check_url_attribute(
    tag: &Tag,
    attribute: &Attribute,
    rel_is_stylesheet: bool,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
) {
    let Some(value) = attribute.value.as_deref() else {
        return;
    };
    match attribute.name.as_str() {
        "src" => {
            if tag.name == "script" {
                sink.error(
                    IssueKind::ExternalScript,
                    attribute.value_start,
                    "单文件要求脚本内联，禁止 <script src>",
                );
            } else {
                check_resource_value(
                    value,
                    attribute.value_start,
                    IssueKind::ExternalResource,
                    config,
                    sink,
                );
            }
        }
        "href" | "xlink:href" => check_href(
            tag,
            value,
            attribute.value_start,
            rel_is_stylesheet,
            config,
            sink,
        ),
        "srcset" => {
            for (url, offset) in parse_srcset(value, attribute.value_start) {
                check_resource_value(url, offset, IssueKind::ExternalResource, config, sink);
            }
        }
        "data" if tag.name == "object" => {
            check_resource_value(
                value,
                attribute.value_start,
                IssueKind::ExternalResource,
                config,
                sink,
            );
        }
        "poster" | "background" => {
            check_resource_value(
                value,
                attribute.value_start,
                IssueKind::ExternalResource,
                config,
                sink,
            );
        }
        "action" | "formaction" => check_form_action(value, attribute.value_start, config, sink),
        _ => {}
    }
}

fn check_href(
    tag: &Tag,
    value: &str,
    offset: usize,
    rel_is_stylesheet: bool,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
) {
    match tag.name.as_str() {
        "a" | "area" => check_navigation_value(value, offset, sink),
        "link" => {
            let kind = if rel_is_stylesheet {
                IssueKind::ExternalStylesheet
            } else {
                IssueKind::ExternalResource
            };
            check_resource_value(value, offset, kind, config, sink);
        }
        "use" | "image" => {
            check_resource_value(value, offset, IssueKind::ExternalResource, config, sink);
        }
        _ => {}
    }
}

fn check_navigation_value(value: &str, offset: usize, sink: &mut IssueSink<'_>) {
    match classify_url(value) {
        UrlKind::Script | UrlKind::VbScript => sink.error(
            IssueKind::DisallowedUrlScheme,
            offset,
            "禁止 javascript:/vbscript: 链接",
        ),
        UrlKind::Data => {
            let normalized = normalize_url(value);
            if normalized.starts_with("data:text/html")
                || normalized.starts_with("data:image/svg+xml")
            {
                sink.error(
                    IssueKind::DisallowedUrlScheme,
                    offset,
                    "禁止可执行的 data: 链接",
                );
            }
        }
        UrlKind::Other(scheme) if matches!(scheme.as_str(), "file" | "blob") => sink.error(
            IssueKind::DisallowedUrlScheme,
            offset,
            format!("禁止 {scheme}: 链接"),
        ),
        _ => {}
    }
}

fn check_form_action(
    value: &str,
    offset: usize,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
) {
    match classify_url(value) {
        UrlKind::Empty | UrlKind::Fragment | UrlKind::Relative => {}
        UrlKind::Script | UrlKind::VbScript | UrlKind::Data => sink.error(
            IssueKind::DisallowedUrlScheme,
            offset,
            "表单 action 使用了危险协议",
        ),
        UrlKind::Http(host) | UrlKind::ProtocolRelative(host) => {
            if !host_allowed(&host, &config.allowed_external_hosts) {
                sink.error(IssueKind::ExternalResource, offset, "表单提交到外部地址");
            }
        }
        UrlKind::Other(_) => sink.error(
            IssueKind::ExternalResource,
            offset,
            "表单 action 使用了不支持的协议",
        ),
    }
}

fn check_resource_value(
    value: &str,
    offset: usize,
    kind: IssueKind,
    config: &ValidatorConfig,
    sink: &mut IssueSink<'_>,
) {
    match classify_url(value) {
        UrlKind::Empty | UrlKind::Fragment => {}
        UrlKind::Data => {
            if !config.allow_data_uris {
                sink.error(kind, offset, "禁止 data: 资源");
            }
        }
        UrlKind::Http(host) | UrlKind::ProtocolRelative(host) => {
            if !host_allowed(&host, &config.allowed_external_hosts) {
                sink.error(kind, offset, format!("引用了外部资源：{value}"));
            }
        }
        UrlKind::Relative => {
            sink.error(kind, offset, format!("引用了单文件之外的相对资源：{value}"))
        }
        UrlKind::Script | UrlKind::VbScript => sink.error(
            IssueKind::DisallowedUrlScheme,
            offset,
            "资源地址使用了危险协议",
        ),
        UrlKind::Other(_) => {
            sink.error(kind, offset, format!("资源地址使用了不支持的协议：{value}"))
        }
    }
}

/// 解析 `srcset` 中的候选 URL（保留 data URI 内的逗号）。
fn parse_srcset(value: &str, base_offset: usize) -> Vec<(&str, usize)> {
    let mut urls = Vec::new();
    let mut rest = value;
    let mut consumed = 0;

    loop {
        let trimmed = rest.trim_start_matches(|ch: char| ch.is_whitespace() || ch == ',');
        consumed += rest.len() - trimmed.len();
        rest = trimmed;
        if rest.is_empty() {
            break;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        urls.push((&rest[..end], base_offset + consumed));
        consumed += end;
        rest = &rest[end..];
        if let Some(comma) = rest.find(',') {
            consumed += comma + 1;
            rest = &rest[comma + 1..];
        } else {
            break;
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srcset_parses_candidates() {
        let urls = parse_srcset("a.png 1x, b.png 2x", 10);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].0, "a.png");
        assert_eq!(urls[1].0, "b.png");
        assert_eq!(urls[1].1, 20);
    }

    #[test]
    fn srcset_keeps_data_uri_comma() {
        let urls = parse_srcset("data:image/png;base64,AAAA 1x", 0);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].0.starts_with("data:image/png"));
    }
}
