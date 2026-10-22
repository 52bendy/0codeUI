//! 显式偏好调研领域模型（REQ-202610-1175，PRD §3.2）。
//!
//! 设计要点：
//! - **A/B 偏好**：一次调研必须同时给出 `selected_style_id` 与 `rejected_style_id`，
//!   且两者不能相同；
//! - **数据最小化**：只接收样式标识、场景、用户 / 会话与时间，不接收自由文本；
//! - **幂等**：客户端可传 `survey_id`，同一用户下重复提交返回已落库记录；
//! - **可关联**：`user_id` + `scene_code` + `occurred_at` 与埋点事件
//!   （`telemetry_events.user_id` / `page_id` / `occurred_at`）使用同一套维度，
//!   客户端传入 `session_id` 时可按会话维度归集。
//!
//! 校验规则与埋点事件采集（REQ-202610-1174）保持一致：标识符只允许
//! ASCII 字母、数字与 `._:-`，时间戳必须落在「90 天前 ~ 未来 5 分钟」内。

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

/// 用户标识最大长度。
pub const MAX_USER_ID_LEN: usize = 64;
/// 会话 / 场景 / 样式标识最大长度。
pub const MAX_IDENTIFIER_LEN: usize = 128;
/// 客户端幂等键最大长度。
pub const MAX_SURVEY_ID_LEN: usize = 64;
/// 时间戳允许的未来偏移（秒）。
pub const MAX_FUTURE_SKEW_SECS: i64 = 300;
/// 时间戳允许的最大历史跨度（秒）：90 天。
pub const MAX_PAST_AGE_SECS: i64 = 90 * 24 * 3600;

/// 调研提交请求。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct SurveyInput {
    /// 用户标识；缺省时取当前鉴权用户。显式提供时必须与鉴权用户一致。
    #[validate(length(min = 1, max = 64, message = "user_id must be 1..=64 characters"))]
    pub user_id: Option<String>,

    /// 会话标识（可选），用于把调研与同一会话的埋点行为归集。
    #[validate(length(min = 1, max = 128, message = "session_id must be 1..=128 characters"))]
    pub session_id: Option<String>,

    /// 场景编码，1~128 字符；与埋点事件的 `page_id` 使用同一场景编码。
    #[validate(length(min = 1, max = 128, message = "scene_code must be 1..=128 characters"))]
    pub scene_code: String,

    /// 用户选择的样式标识，1~128 字符。
    #[validate(length(
        min = 1,
        max = 128,
        message = "selected_style_id must be 1..=128 characters"
    ))]
    pub selected_style_id: String,

    /// 用户拒绝的样式标识，1~128 字符；必须与 `selected_style_id` 不同。
    #[validate(length(
        min = 1,
        max = 128,
        message = "rejected_style_id must be 1..=128 characters"
    ))]
    pub rejected_style_id: String,

    /// 客户端幂等键（可选）；重试同一调研时沿用，服务端据此去重。
    #[validate(length(min = 1, max = 64, message = "survey_id must be 1..=64 characters"))]
    pub survey_id: Option<String>,

    /// 调研发生时间（RFC3339）；缺省为服务端接收时间。
    pub timestamp: Option<DateTime<Utc>>,
}

/// 已落库的调研记录。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TelemetrySurvey {
    /// 服务端主键（`SURV_<uuid>`）。
    pub id: String,
    /// 客户端幂等键（缺省时由服务端生成）。
    pub survey_id: String,
    /// 用户标识。
    pub user_id: String,
    /// 会话标识（可选）。
    pub session_id: Option<String>,
    /// 场景编码。
    pub scene_code: String,
    /// 选择的样式标识。
    pub selected_style_id: String,
    /// 拒绝的样式标识。
    pub rejected_style_id: String,
    /// 调研发生时间（客户端时间，缺省为接收时间）。
    pub timestamp: DateTime<Utc>,
    /// 服务端接收时间。
    pub received_at: DateTime<Utc>,
}

/// 调研提交结果（幂等：重复提交返回已落库记录）。
#[derive(Debug, Clone, Serialize)]
pub struct SurveySubmitResult {
    /// 本次是否新写入。
    pub accepted: bool,
    /// 是否因 `survey_id` 重复而返回已有记录。
    pub duplicate: bool,
    /// 落库记录（新写入或已存在）。
    pub survey: TelemetrySurvey,
}

/// 调研查询过滤条件（用户维度在仓储方法参数中单独传入）。
#[derive(Debug, Clone, Default)]
pub struct SurveyFilter {
    /// 会话标识。
    pub session_id: Option<String>,
    /// 场景编码。
    pub scene_code: Option<String>,
    /// 样式标识；匹配 `selected_style_id` 或 `rejected_style_id`。
    pub style_id: Option<String>,
    /// 起始时间（含）。
    pub from: Option<DateTime<Utc>>,
    /// 结束时间（含）。
    pub to: Option<DateTime<Utc>>,
}

/// 调研明细查询结果（分页）。
#[derive(Debug, Clone, Serialize)]
pub struct SurveyPage {
    /// 当前页记录。
    pub items: Vec<TelemetrySurvey>,
    /// 满足过滤条件的总条数。
    pub total: i64,
    /// 本页大小。
    pub limit: i64,
    /// 本页偏移。
    pub offset: i64,
}

/// 单个样式的选择 / 拒绝统计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurveyStyleStat {
    /// 样式标识。
    pub style_id: String,
    /// 被选择次数。
    pub selected_count: i64,
    /// 被拒绝次数。
    pub rejected_count: i64,
    /// 净偏好（选择次数 - 拒绝次数），供画像引擎（REQ-202610-1176）参考。
    pub net_preference: i64,
}

/// 单个场景的调研量统计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct SurveySceneStat {
    /// 场景编码。
    pub scene_code: String,
    /// 该场景调研次数。
    pub survey_count: i64,
}

/// A/B 样式对统计（供 DPO 数据管道 REQ-202610-1188 消费）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurveyPairStat {
    /// 场景编码。
    pub scene_code: String,
    /// 被选择的样式。
    pub selected_style_id: String,
    /// 被拒绝的样式。
    pub rejected_style_id: String,
    /// 该组合出现次数。
    pub count: i64,
}

/// 调研样本聚合结果。
#[derive(Debug, Clone, Serialize)]
pub struct SurveyAggregate {
    /// 用户标识。
    pub user_id: String,
    /// 起始时间（回显过滤条件）。
    pub from: Option<DateTime<Utc>>,
    /// 结束时间（回显过滤条件）。
    pub to: Option<DateTime<Utc>>,
    /// 调研总数。
    pub total_surveys: i64,
    /// 按样式汇总的选择 / 拒绝次数（按净偏好降序）。
    pub styles: Vec<SurveyStyleStat>,
    /// 按场景汇总的调研量（按数量降序）。
    pub scenes: Vec<SurveySceneStat>,
    /// 按 A/B 样式对汇总（按次数降序，最多 100 条）。
    pub pairs: Vec<SurveyPairStat>,
}

/// 标识符字符集校验：只允许字母、数字与 `._:-`，避免把 URL / 文本带进库。
pub fn validate_identifier(field: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_len {
        return Err(format!("{field} must be 1..={max_len} characters"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return Err(format!(
            "{field} may only contain ASCII letters, digits and `._:-`"
        ));
    }
    Ok(())
}

/// 调研时间必须落在「90 天前 ~ 未来 5 分钟」内（与埋点事件采集一致）。
pub fn validate_timestamp(timestamp: DateTime<Utc>, now: DateTime<Utc>) -> Result<(), String> {
    if timestamp > now + Duration::seconds(MAX_FUTURE_SKEW_SECS) {
        return Err("timestamp must not be more than 5 minutes in the future".to_string());
    }
    if timestamp < now - Duration::seconds(MAX_PAST_AGE_SECS) {
        return Err("timestamp must not be older than 90 days".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_payload() -> serde_json::Value {
        json!({
            "user_id": "u100",
            "session_id": "sess_001",
            "scene_code": "ORDER_DETAIL_PAGE",
            "selected_style_id": "style_modern_dark",
            "rejected_style_id": "style_classic_light",
            "survey_id": "survey_001",
            "timestamp": "2026-10-22T06:00:00Z"
        })
    }

    #[test]
    fn valid_payload_parses_and_validates() {
        let input: SurveyInput = serde_json::from_value(valid_payload()).unwrap();
        assert!(input.validate().is_ok());
        assert_eq!(input.scene_code, "ORDER_DETAIL_PAGE");
    }

    #[test]
    fn missing_required_style_is_rejected() {
        let mut value = valid_payload();
        value.as_object_mut().unwrap().remove("rejected_style_id");
        assert!(serde_json::from_value::<SurveyInput>(value).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value = valid_payload();
        value["page_html"] = json!("<html></html>");
        assert!(serde_json::from_value::<SurveyInput>(value).is_err());
    }

    #[test]
    fn empty_style_is_rejected_by_validator() {
        let mut value = valid_payload();
        value["selected_style_id"] = json!("");
        let input: SurveyInput = serde_json::from_value(value).unwrap();
        assert!(input.validate().is_err());
    }

    #[test]
    fn identifier_charset_is_enforced() {
        assert!(validate_identifier("style", "style_a-1.2:3", 128).is_ok());
        assert!(validate_identifier("style", "style a", 128).is_err());
        assert!(validate_identifier("style", "<script>", 128).is_err());
        assert!(validate_identifier("style", "", 128).is_err());
        assert!(validate_identifier("style", &"a".repeat(129), 128).is_err());
    }

    #[test]
    fn timestamp_window_is_enforced() {
        let now = Utc::now();
        assert!(validate_timestamp(now, now).is_ok());
        assert!(validate_timestamp(now - Duration::seconds(MAX_PAST_AGE_SECS + 1), now).is_err());
        assert!(
            validate_timestamp(now + Duration::seconds(MAX_FUTURE_SKEW_SECS + 1), now).is_err()
        );
    }

    #[test]
    fn survey_serializes_with_timestamp_field() {
        let survey = TelemetrySurvey {
            id: "SURV_1".to_string(),
            survey_id: "survey_001".to_string(),
            user_id: "u100".to_string(),
            session_id: Some("sess_001".to_string()),
            scene_code: "ORDER_DETAIL_PAGE".to_string(),
            selected_style_id: "style_a".to_string(),
            rejected_style_id: "style_b".to_string(),
            timestamp: Utc::now(),
            received_at: Utc::now(),
        };
        let value = serde_json::to_value(&survey).unwrap();
        assert!(value.get("timestamp").is_some());
        assert!(value.get("occurred_at").is_none());
        assert_eq!(value["session_id"], "sess_001");
    }
}
