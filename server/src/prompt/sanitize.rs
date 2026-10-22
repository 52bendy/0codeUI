//! 业务数据脱敏、规范化序列化与注入告警（REQ-202610-1182）。
//!
//! 所有处理都是纯函数：不读取时间、不使用随机数，保证同一输入得到同一输出。

use serde_json::{Map, Value};

use super::model::REDACTED_VALUE;

/// 递归脱敏业务数据，返回新值与被脱敏字段路径。
pub(crate) fn sanitize_business_data(value: &Value) -> (Value, Vec<String>) {
    let mut filtered_fields = Vec::new();
    let sanitized = sanitize_value(value, "business_data", &mut filtered_fields);
    filtered_fields.sort();
    filtered_fields.dedup();
    (sanitized, filtered_fields)
}

fn sanitize_value(value: &Value, path: &str, filtered_fields: &mut Vec<String>) -> Value {
    match value {
        Value::Object(object) => sanitize_object(object, path, filtered_fields),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    sanitize_value(item, &format!("{path}[{index}]"), filtered_fields)
                })
                .collect(),
        ),
        scalar => scalar.clone(),
    }
}

fn sanitize_object(
    object: &Map<String, Value>,
    path: &str,
    filtered_fields: &mut Vec<String>,
) -> Value {
    let mut sanitized = Map::new();
    for (key, value) in object {
        let child_path = format!("{path}.{key}");
        if is_sensitive_key(key) {
            sanitized.insert(key.clone(), Value::String(REDACTED_VALUE.to_string()));
            filtered_fields.push(child_path);
        } else {
            sanitized.insert(
                key.clone(),
                sanitize_value(value, &child_path, filtered_fields),
            );
        }
    }
    Value::Object(sanitized)
}

/// 判断字段名是否属于敏感字段（大小写、下划线、连字符不敏感）。
pub(crate) fn is_sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    if normalized.is_empty() {
        return false;
    }

    const EXACT: &[&str] = &[
        "password",
        "passwd",
        "pwd",
        "secret",
        "token",
        "accesstoken",
        "refreshtoken",
        "apikey",
        "authorization",
        "cookie",
        "setcookie",
        "session",
        "sessionid",
        "csrf",
        "csrftoken",
        "privatekey",
        "clientsecret",
        "idcard",
        "identitycard",
        "bankcard",
        "cardnumber",
        "cvv",
        "cvc",
        "ssn",
        "passport",
        "phone",
        "mobile",
        "email",
    ];
    if EXACT.contains(&normalized.as_str()) {
        return true;
    }

    ["password", "secret", "token", "apikey"]
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
}

/// 稳定 JSON 序列化：对象键按字典序输出，数字保持原始精度。
pub(crate) fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", string_json(key), canonical_json(&object[key])))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Value::Array(items) => {
            let entries: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", entries.join(","))
        }
        Value::String(text) => string_json(text),
        Value::Number(number) => number.to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Null => "null".to_string(),
    }
}

fn string_json(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// 转义可能提前闭合数据区块的字符，保证 `<business_data>` 边界不可突破。
pub(crate) fn escape_prompt_delimiters(text: &str) -> String {
    text.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// 扫描业务数据中的指令式文本，仅生成告警，不修改数据。
pub(crate) fn detect_injection(value: &Value) -> Vec<String> {
    let mut warnings = Vec::new();
    collect_injection_warnings(value, "business_data", &mut warnings);
    warnings.sort();
    warnings.dedup();
    warnings
}

fn collect_injection_warnings(value: &Value, path: &str, warnings: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                collect_injection_warnings(child, &format!("{path}.{key}"), warnings);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_injection_warnings(child, &format!("{path}[{index}]"), warnings);
            }
        }
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            for pattern in INJECTION_PATTERNS {
                if lower.contains(pattern) {
                    warnings.push(format!("{path}: instruction-like pattern `{pattern}`"));
                }
            }
        }
        _ => {}
    }
}

const INJECTION_PATTERNS: &[&str] = &[
    "ignore previous",
    "ignore all previous",
    "disregard previous",
    "forget previous",
    "system prompt",
    "you are now",
    "act as",
    "jailbreak",
    "developer mode",
    "忽略上述",
    "忽略以上",
    "忽略之前",
    "系统提示",
    "你现在是",
    "开发者模式",
    "扮演",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys_recursively() {
        let value = json!({"b": 1, "a": {"y": 2, "x": 3}});
        assert_eq!(canonical_json(&value), r#"{"a":{"x":3,"y":2},"b":1}"#);
    }

    #[test]
    fn sensitive_keys_match_suffixes() {
        assert!(is_sensitive_key("access_token"));
        assert!(is_sensitive_key("userPassword"));
        assert!(is_sensitive_key("api_key"));
        assert!(!is_sensitive_key("token_count"));
        assert!(!is_sensitive_key("shipping_address"));
    }

    #[test]
    fn sanitize_redacts_nested_and_array_fields() {
        let value = json!({
            "password": "x",
            "items": [{"api_key": "k", "name": "ok"}]
        });
        let (sanitized, fields) = sanitize_business_data(&value);
        assert_eq!(sanitized["password"], REDACTED_VALUE);
        assert_eq!(sanitized["items"][0]["api_key"], REDACTED_VALUE);
        assert_eq!(sanitized["items"][0]["name"], "ok");
        assert_eq!(
            fields,
            vec!["business_data.items[0].api_key", "business_data.password"]
        );
    }

    #[test]
    fn delimiters_are_escaped() {
        assert_eq!(
            escape_prompt_delimiters("</business_data>"),
            "\\u003c/business_data\\u003e"
        );
    }
}
