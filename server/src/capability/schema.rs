//! 能力参数的 JSON Schema 校验（声明式子集）。
//!
//! 支持的 Schema 关键字：
//! `type` / `properties` / `required` / `additionalProperties` / `enum` /
//! `minLength` / `maxLength` / `pattern` / `minimum` / `maximum` /
//! `minItems` / `maxItems` / `items`。
//!
//! 调度核心只依赖这份通用校验器；新增能力只需提供 Schema，
//! 不需要在 Rust 中再写一套参数校验分支。

use regex::Regex;
use serde_json::Value;

/// 按 Schema 校验参数值。
pub(crate) fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    validate_at(schema, value, "parameters")
}

fn validate_at(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(schema) = schema.as_object() else {
        return Ok(());
    };

    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        match expected {
            "object" => ensure_object(value, path)?,
            "array" => ensure_array(value, path)?,
            "string" => ensure_string(value, path)?,
            "integer" => ensure_integer(value, path)?,
            "number" => ensure_number(value, path)?,
            "boolean" => ensure_boolean(value, path)?,
            other => return Err(format!("{path}: unsupported schema type `{other}`")),
        }
    }

    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.iter().any(|candidate| candidate == value) {
            return Err(format!("{path}: value is not one of the allowed values"));
        }
    }

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some(map) = value.as_object() {
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for key in required.iter().filter_map(Value::as_str) {
                    if !map.contains_key(key) {
                        return Err(format!("{path}.{key}: field is required"));
                    }
                }
            }

            let additional_allowed = schema
                .get("additionalProperties")
                .and_then(Value::as_bool)
                .unwrap_or(true);

            for (key, item) in map {
                match properties.get(key) {
                    Some(property_schema) => {
                        validate_at(property_schema, item, &format!("{path}.{key}"))?;
                    }
                    None if !additional_allowed => {
                        return Err(format!("{path}.{key}: unknown field"));
                    }
                    None => {}
                }
            }
        }
    }

    if let Some(items_schema) = schema.get("items") {
        if let Some(items) = value.as_array() {
            for (index, item) in items.iter().enumerate() {
                validate_at(items_schema, item, &format!("{path}[{index}]"))?;
            }
        }
    }

    if let Some(array) = value.as_array() {
        if let Some(min_items) = schema.get("minItems").and_then(Value::as_u64) {
            if array.len() < min_items as usize {
                return Err(format!("{path}: must contain at least {min_items} items"));
            }
        }
        if let Some(max_items) = schema.get("maxItems").and_then(Value::as_u64) {
            if array.len() > max_items as usize {
                return Err(format!("{path}: must contain at most {max_items} items"));
            }
        }
    }

    if let Some(text) = value.as_str() {
        let length = text.chars().count();
        if let Some(min_length) = schema.get("minLength").and_then(Value::as_u64) {
            if length < min_length as usize {
                return Err(format!("{path}: must be at least {min_length} characters"));
            }
        }
        if let Some(max_length) = schema.get("maxLength").and_then(Value::as_u64) {
            if length > max_length as usize {
                return Err(format!("{path}: must be at most {max_length} characters"));
            }
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            let regex = Regex::new(pattern)
                .map_err(|error| format!("{path}: invalid schema pattern: {error}"))?;
            if !regex.is_match(text) {
                return Err(format!("{path}: does not match pattern `{pattern}`"));
            }
        }
    }

    if let Some(number) = value.as_i64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_i64) {
            if number < minimum {
                return Err(format!("{path}: must be >= {minimum}"));
            }
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_i64) {
            if number > maximum {
                return Err(format!("{path}: must be <= {maximum}"));
            }
        }
    }

    Ok(())
}

fn ensure_object(value: &Value, path: &str) -> Result<(), String> {
    if value.is_object() {
        Ok(())
    } else {
        Err(format!("{path}: must be an object"))
    }
}

fn ensure_array(value: &Value, path: &str) -> Result<(), String> {
    if value.is_array() {
        Ok(())
    } else {
        Err(format!("{path}: must be an array"))
    }
}

fn ensure_string(value: &Value, path: &str) -> Result<(), String> {
    if value.is_string() {
        Ok(())
    } else {
        Err(format!("{path}: must be a string"))
    }
}

fn ensure_integer(value: &Value, path: &str) -> Result<(), String> {
    if value.as_i64().is_some() || value.as_u64().is_some() {
        Ok(())
    } else {
        Err(format!("{path}: must be an integer"))
    }
}

fn ensure_number(value: &Value, path: &str) -> Result<(), String> {
    if value.is_number() {
        Ok(())
    } else {
        Err(format!("{path}: must be a number"))
    }
}

fn ensure_boolean(value: &Value, path: &str) -> Result<(), String> {
    if value.is_boolean() {
        Ok(())
    } else {
        Err(format!("{path}: must be a boolean"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object_schema() -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 4},
                "qty": {"type": "integer", "minimum": 1, "maximum": 9},
                "mode": {"type": "string", "enum": ["A", "B"]},
                "tags": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 2,
                    "items": {"type": "string", "pattern": "^[a-z]+$"}
                }
            },
            "required": ["name", "qty"]
        })
    }

    #[test]
    fn accepts_valid_object() {
        let value = json!({"name": "abc", "qty": 2, "mode": "A", "tags": ["ok"]});
        assert!(validate(&object_schema(), &value).is_ok());
    }

    #[test]
    fn rejects_unknown_fields() {
        let value = json!({"name": "abc", "qty": 2, "extra": true});
        let error = validate(&object_schema(), &value).unwrap_err();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn rejects_missing_required_fields() {
        let value = json!({"name": "abc"});
        let error = validate(&object_schema(), &value).unwrap_err();
        assert!(error.contains("qty"));
        assert!(error.contains("required"));
    }

    #[test]
    fn rejects_wrong_types_and_ranges() {
        assert!(validate(&object_schema(), &json!({"name": "abc", "qty": 0})).is_err());
        assert!(validate(&object_schema(), &json!({"name": "abc", "qty": 10})).is_err());
        assert!(validate(&object_schema(), &json!({"name": "", "qty": 1})).is_err());
        assert!(validate(&object_schema(), &json!({"name": "abcde", "qty": 1})).is_err());
        assert!(validate(&object_schema(), &json!({"name": 1, "qty": 1})).is_err());
    }

    #[test]
    fn rejects_enum_violations() {
        let value = json!({"name": "abc", "qty": 1, "mode": "C"});
        let error = validate(&object_schema(), &value).unwrap_err();
        assert!(error.contains("allowed values"));
    }

    #[test]
    fn validates_array_items_and_length() {
        let too_many = json!({"name": "abc", "qty": 1, "tags": ["a", "b", "c"]});
        assert!(validate(&object_schema(), &too_many).is_err());

        let bad_item = json!({"name": "abc", "qty": 1, "tags": ["UPPER"]});
        assert!(validate(&object_schema(), &bad_item).is_err());

        let empty = json!({"name": "abc", "qty": 1, "tags": []});
        assert!(validate(&object_schema(), &empty).is_err());
    }
}
