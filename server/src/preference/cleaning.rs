//! 偏好对清洗与质量打分。
//!
//! 入库前统一执行：
//! 1. 文本归一化（换行、首尾空白）；
//! 2. 长度 / 体积限制（prompt 字符数、HTML 字节数、元数据字节数）；
//! 3. PII 脱敏（邮箱、手机号，prompt 与元数据字符串递归处理）；
//! 4. 结构校验（chosen 必须是合法单文件 HTML；rejected 允许非法但会标记）；
//! 5. 质量打分与自动审核状态判定。
//!
//! 打分公式（满分 1.0）：
//! - chosen 合法：0.40
//! - rejected 合法：0.20
//! - prompt ≥ 12 字符：0.15
//! - 溯源完整（模板版本 + chosen 模型）：0.15
//! - 偏好强度 × 0.10
//!
//! 低于 `min_quality` 自动拒绝；达到 `auto_accept_quality` 且无阻断标记
//! （`rejected_html_invalid` / `missing_provenance`）自动通过；其余进入人工审核。

use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::html_validator::validate_single_file_html;
use crate::preference::config::PreferenceConfig;
use crate::preference::model::{PairDraft, PairStatus};

/// 清洗后的偏好对。
#[derive(Debug, Clone)]
pub struct CleanedPair {
    /// 归一化后的 prompt。
    pub prompt: String,
    /// 归一化后的 chosen HTML。
    pub chosen_html: String,
    /// 归一化后的 rejected HTML。
    pub rejected_html: String,
    /// 幂等键。
    pub pair_key: String,
    /// 内容哈希。
    pub content_hash: String,
    /// 质量分。
    pub quality_score: f64,
    /// 质量标记。
    pub quality_flags: Vec<String>,
    /// 自动判定状态。
    pub status: String,
    /// 自动审核备注。
    pub review_note: Option<String>,
    /// 脱敏后的元数据。
    pub metadata: Value,
    /// 偏好发生时间。
    pub occurred_at: DateTime<Utc>,
}

/// 执行清洗、脱敏、打分与状态判定。
pub fn clean_pair(
    draft: &PairDraft,
    user_id: &str,
    config: &PreferenceConfig,
    now: DateTime<Utc>,
) -> Result<CleanedPair, String> {
    let prompt = normalize_text(&draft.prompt);
    if prompt.is_empty() {
        return Err("prompt must not be empty".to_string());
    }
    if prompt.chars().count() > config.max_prompt_chars {
        return Err(format!(
            "prompt must be at most {} characters",
            config.max_prompt_chars
        ));
    }

    let chosen_html = normalize_text(&draft.chosen_html);
    let rejected_html = normalize_text(&draft.rejected_html);
    check_html_size("chosen_html", &chosen_html, config.max_html_bytes)?;
    check_html_size("rejected_html", &rejected_html, config.max_html_bytes)?;
    if chosen_html == rejected_html {
        return Err("chosen_html and rejected_html must differ".to_string());
    }

    let chosen_validation = validate_single_file_html(&chosen_html);
    if !chosen_validation.valid {
        let message = chosen_validation
            .first_error()
            .map(|issue| issue.message.clone())
            .unwrap_or_else(|| "unknown validation error".to_string());
        return Err(format!(
            "chosen_html is not valid single-file HTML: {message}"
        ));
    }
    let rejected_valid = validate_single_file_html(&rejected_html).valid;

    let (prompt, pii_count) = redact_pii(&prompt);
    let mut metadata_pii = 0usize;
    let metadata = sanitize_metadata(draft.metadata.clone(), config, &mut metadata_pii)?;

    let mut flags = Vec::new();
    if pii_count + metadata_pii > 0 {
        flags.push("pii_redacted".to_string());
    }
    if !rejected_valid {
        flags.push("rejected_html_invalid".to_string());
    }
    if prompt.chars().count() < 12 {
        flags.push("short_prompt".to_string());
    }
    if draft.preference_strength < 0.5 {
        flags.push("low_preference_strength".to_string());
    }
    let provenance_complete = draft.prompt_template_version != "unknown"
        && draft
            .chosen_model
            .as_deref()
            .map(|model| !model.trim().is_empty())
            .unwrap_or(false);
    if !provenance_complete {
        flags.push("missing_provenance".to_string());
    }
    flags.sort();
    flags.dedup();

    let quality_score = score_pair(
        rejected_valid,
        prompt.chars().count(),
        provenance_complete,
        draft.preference_strength,
    );
    let (status, review_note) = decide_status(quality_score, &flags, config);

    let content_hash = hash_content(&prompt, &chosen_html, &rejected_html);
    let pair_key = hash_pair_key(user_id, &draft.source, &draft.scene_code, &content_hash);

    Ok(CleanedPair {
        prompt,
        chosen_html,
        rejected_html,
        pair_key,
        content_hash,
        quality_score,
        quality_flags: flags,
        status: status.to_string(),
        review_note,
        metadata,
        occurred_at: draft.occurred_at.unwrap_or(now),
    })
}

/// 归一化文本：统一换行、去除 NUL、裁剪首尾空白。
pub fn normalize_text(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\0', "")
        .trim()
        .to_string()
}

/// 检查 HTML 体积。
fn check_html_size(field: &str, html: &str, max_bytes: usize) -> Result<(), String> {
    if html.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if html.len() > max_bytes {
        return Err(format!("{field} must be at most {max_bytes} bytes"));
    }
    Ok(())
}

/// 质量打分（保留四位小数）。
fn score_pair(
    rejected_valid: bool,
    prompt_chars: usize,
    provenance_complete: bool,
    preference_strength: f64,
) -> f64 {
    let mut score = 0.40;
    if rejected_valid {
        score += 0.20;
    }
    if prompt_chars >= 12 {
        score += 0.15;
    }
    if provenance_complete {
        score += 0.15;
    }
    score += preference_strength.clamp(0.0, 1.0) * 0.10;
    ((score * 10_000.0).round() / 10_000.0).min(1.0)
}

/// 依据质量分与标记决定状态。
fn decide_status(
    quality_score: f64,
    flags: &[String],
    config: &PreferenceConfig,
) -> (&'static str, Option<String>) {
    if quality_score < config.min_quality {
        return (
            PairStatus::Rejected.as_str(),
            Some(format!(
                "auto-rejected: quality_score {quality_score:.4} below {:.4}",
                config.min_quality
            )),
        );
    }
    let blocked = flags.iter().any(|flag| {
        matches!(
            flag.as_str(),
            "rejected_html_invalid" | "missing_provenance"
        )
    });
    if quality_score >= config.auto_accept_quality && !blocked {
        (PairStatus::Accepted.as_str(), None)
    } else {
        (PairStatus::Pending.as_str(), None)
    }
}

/// 计算内容哈希。
pub fn hash_content(prompt: &str, chosen_html: &str, rejected_html: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    hasher.update(b"\n---\n");
    hasher.update(chosen_html.as_bytes());
    hasher.update(b"\n---\n");
    hasher.update(rejected_html.as_bytes());
    hex::encode(hasher.finalize())
}

/// 计算幂等键（同一用户 / 来源 / 场景 / 内容只入库一次）。
pub fn hash_pair_key(user_id: &str, source: &str, scene_code: &str, content_hash: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [user_id, source, scene_code, content_hash] {
        hasher.update(part.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// 计算带盐哈希（用于导出时的用户 / 会话假名化）。
pub fn salted_hash(salt: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

/// 对 prompt 做 PII 脱敏（邮箱 + 中国大陆手机号）。
pub fn redact_pii(input: &str) -> (String, usize) {
    let (without_emails, email_count) = redact_emails(input);
    let (without_phones, phone_count) = redact_phone_numbers(&without_emails);
    (without_phones, email_count + phone_count)
}

/// 邮箱脱敏：按 token 识别 `local@domain.tld`。
fn redact_emails(input: &str) -> (String, usize) {
    let mut output = String::with_capacity(input.len());
    let mut token = String::new();
    let mut count = 0usize;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-' | '@') {
            token.push(ch);
        } else {
            flush_email_token(&mut token, &mut output, &mut count);
            output.push(ch);
        }
    }
    flush_email_token(&mut token, &mut output, &mut count);
    (output, count)
}

/// 输出并清空一个邮箱候选 token。
fn flush_email_token(token: &mut String, output: &mut String, count: &mut usize) {
    if token.is_empty() {
        return;
    }
    let is_email = token
        .split_once('@')
        .map(|(local, domain)| !local.is_empty() && domain.contains('.'))
        .unwrap_or(false);
    if is_email {
        output.push_str("[EMAIL]");
        *count += 1;
    } else {
        output.push_str(token);
    }
    token.clear();
}

/// 手机号脱敏：识别 11 位中国大陆手机号（可带 `+86` / `86` 前缀与分隔符）。
fn redact_phone_numbers(input: &str) -> (String, usize) {
    let chars: Vec<char> = input.chars().collect();
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;
    let mut count = 0usize;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let start = index;
        let mut end = index;
        while end < chars.len() && (chars[end].is_ascii_digit() || matches!(chars[end], '-' | ' '))
        {
            end += 1;
        }
        let candidate: String = chars[start..end].iter().collect();
        let digits: String = candidate.chars().filter(char::is_ascii_digit).collect();
        if is_cn_mobile(&digits) {
            output.push_str("[PHONE]");
            count += 1;
        } else {
            output.push_str(&candidate);
        }
        index = end;
    }
    (output, count)
}

/// 是否为 11 位中国大陆手机号（可带 86 前缀）。
fn is_cn_mobile(digits: &str) -> bool {
    let digits = digits.strip_prefix("86").unwrap_or(digits);
    digits.len() == 11 && digits.starts_with('1') && matches!(digits.as_bytes()[1], b'3'..=b'9')
}

/// 元数据校验与递归脱敏。
fn sanitize_metadata(
    metadata: Value,
    config: &PreferenceConfig,
    pii_count: &mut usize,
) -> Result<Value, String> {
    if !metadata.is_object() {
        return Err("metadata must be a JSON object".to_string());
    }
    let sanitized = redact_json_strings(&metadata, pii_count);
    let size = serde_json::to_vec(&sanitized)
        .map_err(|error| format!("metadata is not serializable: {error}"))?
        .len();
    if size > config.max_metadata_bytes {
        return Err(format!(
            "metadata must be at most {} bytes",
            config.max_metadata_bytes
        ));
    }
    Ok(sanitized)
}

/// 递归脱敏 JSON 字符串值。
fn redact_json_strings(value: &Value, pii_count: &mut usize) -> Value {
    match value {
        Value::String(text) => {
            let (redacted, count) = redact_pii(text);
            *pii_count += count;
            Value::String(redacted)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_json_strings(item, pii_count))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), redact_json_strings(item, pii_count)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preference::model::PairDraft;
    use serde_json::json;

    const VALID: &str =
        "<!DOCTYPE html><html><head><title>t</title></head><body><p>ok</p></body></html>";
    const VALID_ALT: &str =
        "<!DOCTYPE html><html><head><title>t</title></head><body><p>alt</p></body></html>";

    fn draft() -> PairDraft {
        PairDraft {
            prompt: "生成一个订单详情页面，展示商品与金额。".to_string(),
            chosen_html: VALID.to_string(),
            rejected_html: VALID_ALT.to_string(),
            source: "AB_TEST".to_string(),
            scene_code: "ORDER_DETAIL_PAGE".to_string(),
            session_id: None,
            request_id: None,
            survey_id: None,
            prompt_template_version: "prompt-v1".to_string(),
            chosen_model: Some("qwen2.5-coder-7b".to_string()),
            rejected_model: Some("qwen2.5-coder-7b".to_string()),
            chosen_style_id: None,
            rejected_style_id: None,
            pipeline_version: Some("pipeline-v1".to_string()),
            preference_strength: 1.0,
            metadata: json!({}),
            occurred_at: None,
        }
    }

    #[test]
    fn clean_pair_scores_full_quality_as_accepted() {
        let config = PreferenceConfig::default();
        let cleaned = clean_pair(&draft(), "u100", &config, Utc::now()).unwrap();
        assert_eq!(cleaned.quality_score, 1.0);
        assert_eq!(cleaned.status, "ACCEPTED");
        assert!(cleaned.quality_flags.is_empty());
    }

    #[test]
    fn pii_is_redacted_in_prompt_and_metadata() {
        let config = PreferenceConfig::default();
        let mut value = draft();
        value.prompt = "联系 test@example.com 或 13800138000 生成页面".to_string();
        value.metadata = json!({ "note": "备用 13900139000" });
        let cleaned = clean_pair(&value, "u100", &config, Utc::now()).unwrap();
        assert!(cleaned.prompt.contains("[EMAIL]"));
        assert!(cleaned.prompt.contains("[PHONE]"));
        assert!(!cleaned.prompt.contains("13800138000"));
        assert_eq!(cleaned.metadata["note"], "备用 [PHONE]");
        assert!(cleaned.quality_flags.contains(&"pii_redacted".to_string()));
    }

    #[test]
    fn invalid_rejected_html_is_flagged_and_not_auto_accepted() {
        let config = PreferenceConfig::default();
        let mut value = draft();
        value.rejected_html = "<html><body>broken".to_string();
        let cleaned = clean_pair(&value, "u100", &config, Utc::now()).unwrap();
        assert!(cleaned
            .quality_flags
            .contains(&"rejected_html_invalid".to_string()));
        assert_eq!(cleaned.status, "PENDING");
    }

    #[test]
    fn identical_outputs_are_rejected() {
        let config = PreferenceConfig::default();
        let mut value = draft();
        value.rejected_html = value.chosen_html.clone();
        assert!(clean_pair(&value, "u100", &config, Utc::now()).is_err());
    }

    #[test]
    fn low_quality_pair_is_auto_rejected() {
        let config = PreferenceConfig::default();
        let mut value = draft();
        value.prompt = "短".to_string();
        value.prompt_template_version = "unknown".to_string();
        value.chosen_model = None;
        value.preference_strength = 0.0;
        value.rejected_html = "<html><body>broken".to_string();
        let cleaned = clean_pair(&value, "u100", &config, Utc::now()).unwrap();
        assert!(cleaned.quality_score < config.min_quality);
        assert_eq!(cleaned.status, "REJECTED");
    }

    #[test]
    fn same_content_has_same_hash_across_users_but_different_pair_key() {
        let config = PreferenceConfig::default();
        let first = clean_pair(&draft(), "u100", &config, Utc::now()).unwrap();
        let second = clean_pair(&draft(), "u200", &config, Utc::now()).unwrap();
        assert_eq!(first.content_hash, second.content_hash);
        assert_ne!(first.pair_key, second.pair_key);
    }
}
