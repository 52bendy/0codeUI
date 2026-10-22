//! 容错 HTML 词法分析器：输出带位置信息的 token，并记录词法级错误。
//!
//! 只做校验所需的解析：标签、属性、注释、DOCTYPE 与 `script` / `style`
//! 等原始文本元素。不做 DOM 树构建，结构校验由 `checks` 模块完成。

use crate::html_validator::model::IssueKind;
use crate::html_validator::sink::IssueSink;

/// 标签属性。
#[derive(Debug, Clone)]
pub(crate) struct Attribute {
    /// 小写属性名。
    pub name: String,
    /// 属性名起始偏移。
    pub name_start: usize,
    /// 属性值（保留原始文本，使用前需做实体解码）。
    pub value: Option<String>,
    /// 属性值起始偏移。
    pub value_start: usize,
}

/// 开始标签。
#[derive(Debug, Clone)]
pub(crate) struct Tag {
    /// 小写标签名。
    pub name: String,
    /// 属性列表。
    pub attributes: Vec<Attribute>,
    /// 是否使用 `/>` 自闭合语法。
    pub self_closing: bool,
    /// 标签起始偏移。
    pub start: usize,
}

/// token 类型。
#[derive(Debug, Clone)]
pub(crate) enum TokenKind {
    /// `<!DOCTYPE html>`。
    Doctype { name: String },
    /// 注释（内容无需校验）。
    Comment,
    /// 开始标签。
    StartTag(Tag),
    /// 结束标签。
    EndTag { name: String, start: usize },
    /// `script` / `style` 等原始文本内容。
    RawText {
        tag: String,
        content_start: usize,
        content: String,
    },
    /// 普通文本。
    Text { text: String },
}

/// 带起始位置的 token。
#[derive(Debug, Clone)]
pub(crate) struct Token {
    /// token 类型。
    pub kind: TokenKind,
    /// token 起始偏移。
    pub start: usize,
}

const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style", "textarea", "title", "xmp", "plaintext"];

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// 是否为 HTML 空元素。
pub(crate) fn is_void_element(name: &str) -> bool {
    VOID_ELEMENTS.contains(&name)
}

/// 是否为原始文本元素。
pub(crate) fn is_raw_text_element(name: &str) -> bool {
    RAW_TEXT_ELEMENTS.contains(&name)
}

/// 执行词法分析。
pub(crate) fn tokenize<'a>(input: &'a str, sink: &mut IssueSink<'a>) -> Vec<Token> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < bytes.len() {
        let Some(relative) = input[pos..].find('<') else {
            push_text(&mut tokens, input, pos, bytes.len());
            break;
        };
        let lt = pos + relative;
        if lt > pos {
            push_text(&mut tokens, input, pos, lt);
        }

        if input[lt..].starts_with("<!--") {
            let (token, next) = parse_comment(input, lt, sink);
            tokens.push(token);
            pos = next;
        } else if is_doctype_start(input, lt) {
            let (token, next) = parse_doctype(input, lt, sink);
            tokens.push(token);
            pos = next;
        } else if input[lt..].starts_with("<!") || input[lt..].starts_with("<?") {
            let next = skip_declaration(input, lt, sink);
            tokens.push(Token {
                kind: TokenKind::Comment,
                start: lt,
            });
            pos = next;
        } else if input[lt..].starts_with("</") {
            let (token, next) = parse_end_tag(input, lt, sink);
            tokens.push(token);
            pos = next;
        } else if bytes.get(lt + 1).is_some_and(u8::is_ascii_alphabetic) {
            let (tag, next) = parse_start_tag(input, lt, sink);
            let raw = is_raw_text_element(&tag.name);
            tokens.push(Token {
                kind: TokenKind::StartTag(tag.clone()),
                start: tag.start,
            });
            if raw {
                let (token, next) = parse_raw_text(input, &tag, next, sink);
                tokens.push(token);
                pos = next;
            } else {
                pos = next;
            }
        } else {
            push_text(&mut tokens, input, lt, lt + 1);
            pos = lt + 1;
        }
    }

    tokens
}

fn push_text(tokens: &mut Vec<Token>, input: &str, start: usize, end: usize) {
    if end > start {
        tokens.push(Token {
            kind: TokenKind::Text {
                text: input[start..end].to_string(),
            },
            start,
        });
    }
}

fn is_doctype_start(input: &str, at: usize) -> bool {
    let bytes = input.as_bytes();
    if bytes.len() < at + 10 {
        return false;
    }
    if !bytes[at..at + 9].eq_ignore_ascii_case(b"<!doctype") {
        return false;
    }
    matches!(bytes[at + 9], b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

fn parse_doctype<'a>(input: &'a str, start: usize, sink: &mut IssueSink<'a>) -> (Token, usize) {
    let body_start = start + 9;
    let Some(relative) = input[body_start..].find('>') else {
        sink.error(IssueKind::UnterminatedTag, start, "DOCTYPE 未闭合");
        return (
            Token {
                kind: TokenKind::Doctype {
                    name: String::new(),
                },
                start,
            },
            input.len(),
        );
    };
    let body = input[body_start..body_start + relative].trim();
    let name = body
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.is_empty() {
        sink.error(IssueKind::InvalidDoctype, start, "DOCTYPE 缺少名称");
    }
    (
        Token {
            kind: TokenKind::Doctype { name },
            start,
        },
        body_start + relative + 1,
    )
}

fn parse_comment<'a>(input: &'a str, start: usize, sink: &mut IssueSink<'a>) -> (Token, usize) {
    let content_start = start + 4;
    let Some(relative) = input[content_start..].find("-->") else {
        sink.error(IssueKind::UnterminatedComment, start, "注释未闭合");
        return (
            Token {
                kind: TokenKind::Comment,
                start,
            },
            input.len(),
        );
    };
    (
        Token {
            kind: TokenKind::Comment,
            start,
        },
        content_start + relative + 3,
    )
}

fn skip_declaration<'a>(input: &'a str, start: usize, sink: &mut IssueSink<'a>) -> usize {
    sink.error(
        IssueKind::InvalidTagSyntax,
        start,
        "不支持的处理指令 / 声明",
    );
    match input[start..].find('>') {
        Some(relative) => start + relative + 1,
        None => input.len(),
    }
}

fn parse_end_tag<'a>(input: &'a str, start: usize, sink: &mut IssueSink<'a>) -> (Token, usize) {
    let bytes = input.as_bytes();
    let mut index = start + 2;
    if !bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
        sink.error(IssueKind::InvalidTagSyntax, start, "结束标签缺少标签名");
        return (
            Token {
                kind: TokenKind::Comment,
                start,
            },
            skip_to_gt(bytes, index),
        );
    }

    let name_start = index;
    index = scan_tag_name(bytes, index);
    let name = input[name_start..index].to_ascii_lowercase();
    index = skip_whitespace(bytes, index);
    if bytes.get(index) == Some(&b'/') {
        sink.error(IssueKind::InvalidTagSyntax, index, "结束标签不允许自闭合");
        index += 1;
        index = skip_whitespace(bytes, index);
    }
    if index >= bytes.len() {
        sink.error(IssueKind::UnterminatedTag, start, "结束标签未闭合");
        return (
            Token {
                kind: TokenKind::EndTag { name, start },
                start,
            },
            bytes.len(),
        );
    }
    if bytes[index] != b'>' {
        sink.error(IssueKind::InvalidTagSyntax, index, "结束标签语法非法");
        index = skip_to_gt(bytes, index);
    } else {
        index += 1;
    }
    (
        Token {
            kind: TokenKind::EndTag { name, start },
            start,
        },
        index,
    )
}

fn parse_start_tag<'a>(input: &'a str, start: usize, sink: &mut IssueSink<'a>) -> (Tag, usize) {
    let bytes = input.as_bytes();
    let name_start = start + 1;
    let mut index = scan_tag_name(bytes, name_start);
    let name = input[name_start..index].to_ascii_lowercase();

    if index < bytes.len() && !is_tag_delimiter(bytes[index]) {
        sink.error(IssueKind::InvalidTagName, index, "标签名包含非法字符");
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b'>' | b'/')
        {
            index += 1;
        }
    }

    let mut attributes = Vec::new();
    let mut self_closing = false;
    let end;

    loop {
        index = skip_whitespace(bytes, index);
        if index >= bytes.len() {
            sink.error(IssueKind::UnterminatedTag, start, "标签未闭合");
            end = bytes.len();
            break;
        }
        match bytes[index] {
            b'>' => {
                index += 1;
                end = index;
                break;
            }
            b'/' if bytes.get(index + 1) == Some(&b'>') => {
                self_closing = true;
                index += 2;
                end = index;
                break;
            }
            b'<' => {
                sink.error(
                    IssueKind::InvalidTagSyntax,
                    index,
                    "标签内部出现未转义的 '<'",
                );
                end = index;
                break;
            }
            _ => {}
        }

        let (attribute, next) = parse_attribute(input, index, start, sink);
        index = next;
        if let Some(attribute) = attribute {
            if attributes
                .iter()
                .any(|existing: &Attribute| existing.name == attribute.name)
            {
                sink.error(
                    IssueKind::DuplicateAttribute,
                    attribute.name_start,
                    format!("属性 '{}' 重复", attribute.name),
                );
            }
            attributes.push(attribute);
        }
    }

    (
        Tag {
            name,
            attributes,
            self_closing,
            start,
        },
        end,
    )
}

fn parse_attribute<'a>(
    input: &'a str,
    start: usize,
    tag_start: usize,
    sink: &mut IssueSink<'a>,
) -> (Option<Attribute>, usize) {
    let bytes = input.as_bytes();
    let name_start = start;
    let mut index = start;
    while index < bytes.len() && !is_attribute_name_end(bytes[index]) {
        index += 1;
    }
    if index == name_start {
        sink.error(IssueKind::InvalidAttributeName, index, "属性名缺失");
        return (None, index + 1);
    }

    let raw_name = &input[name_start..index];
    if !is_valid_attribute_name(raw_name) {
        sink.error(
            IssueKind::InvalidAttributeName,
            name_start,
            format!("属性名 '{}' 非法", raw_name),
        );
    }
    let name = raw_name.to_ascii_lowercase();
    let after_name = skip_whitespace(bytes, index);

    let mut value = None;
    let mut value_start = after_name;
    if bytes.get(after_name) == Some(&b'=') {
        index = skip_whitespace(bytes, after_name + 1);
        let quoted = bytes
            .get(index)
            .is_some_and(|byte| *byte == b'"' || *byte == b'\'');
        value_start = if quoted { index + 1 } else { index };
        let (parsed, next) = parse_attribute_value(input, index, tag_start, sink);
        value = parsed;
        index = next;
    }

    (
        Some(Attribute {
            name,
            name_start,
            value,
            value_start,
        }),
        index,
    )
}

fn parse_attribute_value<'a>(
    input: &'a str,
    start: usize,
    tag_start: usize,
    sink: &mut IssueSink<'a>,
) -> (Option<String>, usize) {
    let bytes = input.as_bytes();
    if start >= bytes.len() {
        sink.error(IssueKind::UnterminatedTag, tag_start, "属性值未闭合");
        return (None, bytes.len());
    }

    if matches!(bytes[start], b'"' | b'\'') {
        let quote = bytes[start];
        let value_start = start + 1;
        let mut index = value_start;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        if index >= bytes.len() {
            sink.error(IssueKind::UnterminatedTag, tag_start, "属性值引号未闭合");
            return (Some(input[value_start..].to_string()), bytes.len());
        }
        return (Some(input[value_start..index].to_string()), index + 1);
    }

    let mut index = start;
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && !matches!(bytes[index], b'>' | b'<')
    {
        index += 1;
    }
    if index == start {
        sink.error(IssueKind::InvalidTagSyntax, start, "属性值缺失");
        return (None, index);
    }
    (Some(input[start..index].to_string()), index)
}

fn parse_raw_text<'a>(
    input: &'a str,
    tag: &Tag,
    from: usize,
    sink: &mut IssueSink<'a>,
) -> (Token, usize) {
    if tag.name == "plaintext" {
        sink.error(
            IssueKind::UnterminatedRawText,
            tag.start,
            "<plaintext> 不允许使用",
        );
        return (raw_token(tag, input, from), input.len());
    }

    match find_raw_end(input, from, &tag.name) {
        Some(end) => (
            Token {
                kind: TokenKind::RawText {
                    tag: tag.name.clone(),
                    content_start: from,
                    content: input[from..end].to_string(),
                },
                start: from,
            },
            end,
        ),
        None => {
            sink.error(
                IssueKind::UnterminatedRawText,
                tag.start,
                format!("<{}> 未闭合", tag.name),
            );
            (raw_token(tag, input, from), input.len())
        }
    }
}

fn raw_token(tag: &Tag, input: &str, from: usize) -> Token {
    Token {
        kind: TokenKind::RawText {
            tag: tag.name.clone(),
            content_start: from,
            content: input[from..].to_string(),
        },
        start: from,
    }
}

fn find_raw_end(input: &str, from: usize, name: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let needle = name.as_bytes();
    let mut index = from;
    while index + needle.len() + 2 <= bytes.len() {
        if bytes[index] == b'<'
            && bytes[index + 1] == b'/'
            && bytes[index + 2..index + 2 + needle.len()].eq_ignore_ascii_case(needle)
        {
            let after = index + 2 + needle.len();
            if after >= bytes.len()
                || bytes[after].is_ascii_whitespace()
                || matches!(bytes[after], b'>' | b'/')
            {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn scan_tag_name(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'-' | b':' | b'_'))
    {
        index += 1;
    }
    index
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn skip_to_gt(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'>' {
        index += 1;
    }
    if index < bytes.len() {
        index + 1
    } else {
        bytes.len()
    }
}

fn is_tag_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/')
}

fn is_attribute_name_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'=' | b'>' | b'/' | b'<' | b'"' | b'\'')
}

fn is_valid_attribute_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            !byte.is_ascii_control() && !matches!(byte, b'"' | b'\'' | b'<' | b'>' | b'=' | b'`')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_validator::source::SourceMap;

    fn lex(input: &str) -> (Vec<Token>, Vec<IssueKind>) {
        let map = SourceMap::new(input);
        let mut sink = IssueSink::new(&map, 100);
        let tokens = tokenize(input, &mut sink);
        (
            tokens,
            sink.finish().iter().map(|issue| issue.kind).collect(),
        )
    }

    #[test]
    fn tokenizes_doctype_and_tags() {
        let (tokens, issues) = lex("<!DOCTYPE html><html><body><br/></body></html>");
        assert!(issues.is_empty(), "{issues:?}");
        assert!(matches!(tokens[0].kind, TokenKind::Doctype { .. }));
        assert!(matches!(tokens[1].kind, TokenKind::StartTag(_)));
        let br = tokens
            .iter()
            .find_map(|token| match &token.kind {
                TokenKind::StartTag(tag) if tag.name == "br" => Some(tag),
                _ => None,
            })
            .expect("br tag");
        assert!(br.self_closing);
    }

    #[test]
    fn script_content_is_raw_text() {
        let (tokens, issues) = lex("<script>if (1 < 2) { x = '</div>'; }</script>");
        assert!(issues.is_empty(), "{issues:?}");
        let raw = tokens
            .iter()
            .find_map(|token| match &token.kind {
                TokenKind::RawText { tag, content, .. } if tag == "script" => Some(content),
                _ => None,
            })
            .expect("raw text");
        assert!(raw.contains("1 < 2"));
    }

    #[test]
    fn unterminated_comment_is_reported() {
        let (_, issues) = lex("<!-- no end");
        assert_eq!(issues, vec![IssueKind::UnterminatedComment]);
    }

    #[test]
    fn quoted_attribute_may_contain_gt() {
        let (tokens, issues) = lex("<div title=\"a > b\">");
        assert!(issues.is_empty(), "{issues:?}");
        let tag = tokens
            .iter()
            .find_map(|token| match &token.kind {
                TokenKind::StartTag(tag) => Some(tag),
                _ => None,
            })
            .expect("tag");
        assert_eq!(tag.attributes[0].value.as_deref(), Some("a > b"));
    }
}
