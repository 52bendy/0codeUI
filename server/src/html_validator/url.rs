//! URL 分类、HTML 实体解码与外部主机白名单。

/// URL 分类结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UrlKind {
    /// 空值。
    Empty,
    /// 页内锚点（`#...`）。
    Fragment,
    /// 内联 data URI。
    Data,
    /// 绝对 http(s) 地址，携带主机名。
    Http(String),
    /// 协议相对地址（`//host/...`），携带主机名。
    ProtocolRelative(String),
    /// 相对路径（单文件内无法解析）。
    Relative,
    /// `javascript:` 协议。
    Script,
    /// `vbscript:` 协议。
    VbScript,
    /// 其他协议（`mailto:` / `blob:` 等）。
    Other(String),
}

/// 解码属性值中的 HTML 实体，避免 `java&#115;cript:` 之类绕过。
pub(crate) fn decode_html_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(index) = rest.find('&') {
        output.push_str(&rest[..index]);
        let entity = &rest[index..];

        if let Some((decoded, consumed)) = decode_entity(entity) {
            output.push(decoded);
            rest = &entity[consumed..];
        } else {
            output.push('&');
            rest = &entity[1..];
        }
    }
    output.push_str(rest);
    output
}

fn decode_entity(entity: &str) -> Option<(char, usize)> {
    let bytes = entity.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'&' {
        return None;
    }

    if bytes[1] == b'#' {
        let (radix, digits_start) = match bytes.get(2) {
            Some(b'x') | Some(b'X') => (16, 3),
            _ => (10, 2),
        };
        let mut end = digits_start;
        while end < bytes.len() && (bytes[end] as char).is_digit(radix) {
            end += 1;
        }
        if end == digits_start {
            return None;
        }
        let digits = &entity[digits_start..end];
        let code = u32::from_str_radix(digits, radix).ok()?;
        let consumed = if bytes.get(end) == Some(&b';') {
            end + 1
        } else {
            end
        };
        return char::from_u32(code).map(|ch| (ch, consumed));
    }

    let semicolon = entity.find(';')?;
    let name = &entity[1..semicolon];
    let decoded = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "colon" => ':',
        "tab" => '\t',
        "newline" => '\n',
        "nbsp" => '\u{a0}',
        "sol" => '/',
        "quest" => '?',
        "num" => '#',
        "equals" => '=',
        "period" => '.',
        _ => return None,
    };
    Some((decoded, semicolon + 1))
}

/// 归一化 URL：解码实体并去掉空白 / 控制字符，便于协议识别。
pub(crate) fn normalize_url(value: &str) -> String {
    decode_html_entities(value)
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace() && !ch.is_control())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// 对 URL 分类。
pub(crate) fn classify_url(value: &str) -> UrlKind {
    let normalized = normalize_url(value);
    if normalized.is_empty() {
        return UrlKind::Empty;
    }
    if normalized.starts_with('#') {
        return UrlKind::Fragment;
    }
    if normalized.starts_with("data:") {
        return UrlKind::Data;
    }
    if normalized.starts_with("javascript:") {
        return UrlKind::Script;
    }
    if normalized.starts_with("vbscript:") {
        return UrlKind::VbScript;
    }
    if let Some(rest) = normalized.strip_prefix("//") {
        return UrlKind::ProtocolRelative(host_of(rest));
    }
    if let Some(rest) = normalized
        .strip_prefix("http://")
        .or_else(|| normalized.strip_prefix("https://"))
    {
        return UrlKind::Http(host_of(rest));
    }
    if let Some((scheme, _)) = normalized.split_once(':') {
        if scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
            && !scheme.is_empty()
        {
            return UrlKind::Other(scheme.to_string());
        }
    }
    UrlKind::Relative
}

fn host_of(authority: &str) -> String {
    let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(stripped) = authority.strip_prefix('[') {
        return stripped
            .split(']')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
    }
    authority
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// 判断主机是否命中白名单（支持 `*.example.com` 通配）。
pub(crate) fn host_allowed(host: &str, whitelist: &[String]) -> bool {
    whitelist.iter().any(|pattern| {
        let pattern = pattern.trim().to_ascii_lowercase();
        if let Some(suffix) = pattern.strip_prefix("*.") {
            host == suffix || host.ends_with(&format!(".{suffix}"))
        } else {
            host == pattern
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_numeric_and_named_entities() {
        assert_eq!(decode_html_entities("java&#115;cript"), "javascript");
        assert_eq!(decode_html_entities("java&#x73;cript"), "javascript");
        assert_eq!(decode_html_entities("a&amp;b&colon;c"), "a&b:c");
        assert_eq!(
            decode_html_entities("plain &unknown; text"),
            "plain &unknown; text"
        );
    }

    #[test]
    fn classifies_urls() {
        assert_eq!(classify_url(""), UrlKind::Empty);
        assert_eq!(classify_url("#top"), UrlKind::Fragment);
        assert_eq!(classify_url("data:image/png;base64,AA"), UrlKind::Data);
        assert_eq!(classify_url("javascript:alert(1)"), UrlKind::Script);
        assert_eq!(classify_url("JaVaScRiPt:alert(1)"), UrlKind::Script);
        assert_eq!(
            classify_url("https://cdn.example.com/a.png"),
            UrlKind::Http("cdn.example.com".into())
        );
        assert_eq!(
            classify_url("//cdn.example.com/a.png"),
            UrlKind::ProtocolRelative("cdn.example.com".into())
        );
        assert_eq!(classify_url("images/a.png"), UrlKind::Relative);
        assert_eq!(
            classify_url("mailto:a@b.c"),
            UrlKind::Other("mailto".into())
        );
    }

    #[test]
    fn normalizes_obfuscated_schemes() {
        assert_eq!(classify_url("java\nscript:alert(1)"), UrlKind::Script);
        assert_eq!(classify_url("java&#115;cript:alert(1)"), UrlKind::Script);
        assert_eq!(classify_url(" javascript:\talert(1)"), UrlKind::Script);
    }

    #[test]
    fn matches_host_whitelist() {
        let whitelist = vec!["cdn.example.com".to_string(), "*.trusted.test".to_string()];
        assert!(host_allowed("cdn.example.com", &whitelist));
        assert!(host_allowed("a.trusted.test", &whitelist));
        assert!(host_allowed("trusted.test", &whitelist));
        assert!(!host_allowed("evil.example.com", &whitelist));
    }
}
