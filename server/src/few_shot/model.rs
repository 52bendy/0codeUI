//! Few-Shot 组件片段检索的领域模型（REQ-202610-1183）。
//!
//! 片段库同时服务两个场景：
//! - **写入侧**：历史生成结果、人工策展与内置模板登记为可检索片段；
//! - **读取侧**：按场景 / 用户画像 / 语义相似度排序，渲染成受 token 预算约束的
//!   Prompt 片段，供 14 Prompt 拼装器注入。
//!
//! 所有用户输入都在本模块完成长度、字符集与范围校验；片段 HTML 额外做
//! 危险标记检查，避免把可执行内容带进 Prompt 或预览页面。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::profile::Theme;

/// 场景编码最大长度。
pub const SCENE_CODE_MAX_CHARS: usize = 64;
/// 片段标题最大长度。
pub const TITLE_MAX_CHARS: usize = 200;
/// 片段 HTML 最大长度（字符）。
pub const SNIPPET_HTML_MAX_CHARS: usize = 20_000;
/// 片段纯文本最大长度（字符）。
pub const SNIPPET_TEXT_MAX_CHARS: usize = 4_000;
/// 检索查询最大长度（字符）。
pub const QUERY_MAX_CHARS: usize = 2_000;
/// 基础 Prompt 最大长度（字符）。
pub const BASE_PROMPT_MAX_CHARS: usize = 32_000;
/// 单个片段最多标签数。
pub const TAG_MAX_COUNT: usize = 12;
/// 单个标签最大长度。
pub const TAG_MAX_CHARS: usize = 32;
/// 默认返回片段数。
pub const DEFAULT_TOP_K: usize = 3;
/// 默认 token 预算。
pub const DEFAULT_TOKEN_BUDGET: usize = 1_200;
/// 默认最低相关性阈值。
pub const DEFAULT_MIN_SCORE: f64 = 0.15;
/// 默认单片段最大字符数。
pub const DEFAULT_MAX_SNIPPET_CHARS: usize = 4_000;
/// 默认候选集上限。
pub const DEFAULT_CANDIDATE_LIMIT: usize = 200;

/// 片段来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SnippetSource {
    /// 历史生成结果。
    Generated,
    /// 人工策展。
    Curated,
    /// 内置模板。
    Template,
}

impl SnippetSource {
    /// 数据库字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "GENERATED",
            Self::Curated => "CURATED",
            Self::Template => "TEMPLATE",
        }
    }

    /// 从数据库字符串解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "GENERATED" => Some(Self::Generated),
            "CURATED" => Some(Self::Curated),
            "TEMPLATE" => Some(Self::Template),
            _ => None,
        }
    }
}

/// 片段适配的主题；`Any` 表示明暗主题都适用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnippetTheme {
    /// 不限定主题。
    #[default]
    Any,
    /// 明亮主题。
    Light,
    /// 暗黑主题。
    Dark,
}

impl SnippetTheme {
    /// 数据库字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// 从数据库字符串解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "any" => Some(Self::Any),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    /// 与画像主题的匹配度：不限定 = 1.0，匹配 = 1.0，不匹配 = 0.0。
    pub fn match_score(self, theme: Theme) -> f64 {
        match self {
            Self::Any => 1.0,
            Self::Light if theme == Theme::Light => 1.0,
            Self::Dark if theme == Theme::Dark => 1.0,
            _ => 0.0,
        }
    }
}

/// 片段的风格特征（用于与用户画像做亲和度匹配）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnippetStyle {
    /// 适用主题。
    #[serde(default)]
    pub theme: SnippetTheme,
    /// 字号缩放。
    #[serde(default)]
    pub font_scale: Option<f64>,
    /// 密度。
    #[serde(default)]
    pub density: Option<f64>,
    /// 圆角（px）。
    #[serde(default)]
    pub radius: Option<f64>,
    /// 间距缩放。
    #[serde(default)]
    pub spacing: Option<f64>,
}

/// 组件片段（持久化领域对象）。
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentSnippet {
    /// 主键 `FSP_<uuid>`。
    pub id: String,
    /// 来源类型。
    pub source_type: SnippetSource,
    /// 来源引用（生成任务 ID / 模板版本 / 策展批次）。
    pub source_ref: Option<String>,
    /// 所属场景编码。
    pub scene_code: String,
    /// 标题。
    pub title: String,
    /// 片段 HTML。
    pub snippet_html: String,
    /// 用于检索的纯文本。
    pub snippet_text: String,
    /// 标签。
    pub tags: Vec<String>,
    /// 风格特征。
    pub style: SnippetStyle,
    /// 显式满意度（0.0 ~ 1.0）。
    pub satisfaction: f64,
    /// 隐式满意度（使用次数 / 正负反馈推导）。
    pub implicit_score: f64,
    /// 使用次数（反馈 / 采纳计数）。
    pub usage_count: u64,
    /// 正向反馈数。
    pub positive_feedback: u64,
    /// 负向反馈数。
    pub negative_feedback: u64,
    /// 是否参与检索。
    pub enabled: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

impl ComponentSnippet {
    /// 综合质量分：显式满意度为主，隐式反馈为辅。
    pub fn quality_score(&self) -> f64 {
        let feedback = (self.positive_feedback as f64 + 1.0)
            / ((self.positive_feedback + self.negative_feedback) as f64 + 2.0);
        let implicit = (0.7 * self.implicit_score + 0.3 * feedback).clamp(0.0, 1.0);
        (0.7 * self.satisfaction + 0.3 * implicit).clamp(0.0, 1.0)
    }
}

/// 检索配置（由 [`AppConfig`] 派生，测试可单独覆盖）。
#[derive(Debug, Clone)]
pub struct FewShotConfig {
    /// 功能总开关。
    pub enabled: bool,
    /// 返回片段数上限。
    pub top_k: usize,
    /// Prompt 片段 token 预算。
    pub token_budget: usize,
    /// 最低相关性阈值，低于该值安全降级。
    pub min_score: f64,
    /// 单片段最大字符数。
    pub max_snippet_chars: usize,
    /// 单场景候选集上限。
    pub candidate_limit: usize,
    /// 写接口管理令牌；设置后 `POST` 必须携带 `X-Few-Shot-Token`。
    pub admin_token: Option<String>,
}

impl Default for FewShotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            top_k: DEFAULT_TOP_K,
            token_budget: DEFAULT_TOKEN_BUDGET,
            min_score: DEFAULT_MIN_SCORE,
            max_snippet_chars: DEFAULT_MAX_SNIPPET_CHARS,
            candidate_limit: DEFAULT_CANDIDATE_LIMIT,
            admin_token: None,
        }
    }
}

impl FewShotConfig {
    /// 从应用配置派生。
    pub fn from_app(config: &AppConfig) -> Self {
        Self {
            enabled: config.few_shot_enabled,
            top_k: config.few_shot_top_k,
            token_budget: config.few_shot_token_budget,
            min_score: config.few_shot_min_score,
            max_snippet_chars: config.few_shot_max_snippet_chars,
            candidate_limit: config.few_shot_candidate_limit,
            admin_token: config.few_shot_admin_token.clone(),
        }
    }
}

/// 检索查询。
#[derive(Debug, Clone)]
pub struct FewShotQuery {
    /// 场景编码。
    pub scene_code: String,
    /// 语义查询文本（可为空，此时以画像与质量排序）。
    pub query: Option<String>,
    /// 覆盖默认 top_k。
    pub top_k: Option<usize>,
    /// 覆盖默认 token 预算。
    pub token_budget: Option<usize>,
    /// 可选的基础 Prompt；提供时返回注入后的完整 Prompt。
    pub base_prompt: Option<String>,
}

/// 画像来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    /// 使用 08 用户画像引擎的持久化画像。
    Profile,
    /// 使用 06 埋点 / 07 调研样本即时推导。
    Signals,
    /// 冷启动，无可用偏好。
    ColdStart,
}

/// 降级原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradeReason {
    /// 功能被配置关闭。
    Disabled,
    /// 场景片段库为空。
    EmptyLibrary,
    /// 最高相关性低于阈值。
    LowConfidence,
    /// token 预算无法容纳任何片段。
    TokenBudgetExceeded,
}

/// 单条检索结果。
#[derive(Debug, Clone, Serialize)]
pub struct RetrievedSnippet {
    /// 片段 ID。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 场景编码。
    pub scene_code: String,
    /// 来源类型。
    pub source_type: SnippetSource,
    /// 标签。
    pub tags: Vec<String>,
    /// 风格特征。
    pub style: SnippetStyle,
    /// 显式满意度。
    pub satisfaction: f64,
    /// 综合得分（0.0 ~ 1.0）。
    pub score: f64,
    /// 语义相似度。
    pub semantic_score: f64,
    /// 画像亲和度。
    pub profile_score: f64,
    /// 质量分。
    pub quality_score: f64,
    /// 片段估算 token 数。
    pub token_estimate: usize,
    /// 片段 HTML。
    pub snippet_html: String,
}

/// 检索结果。
#[derive(Debug, Clone, Serialize)]
pub struct FewShotResult {
    /// 场景编码。
    pub scene_code: String,
    /// 是否降级为「无 few-shot」。
    pub degraded: bool,
    /// 降级原因。
    pub degraded_reason: Option<DegradeReason>,
    /// 该场景的候选片段总数。
    pub candidate_count: usize,
    /// 命中的片段（已按得分降序）。
    pub snippets: Vec<RetrievedSnippet>,
    /// 可直接注入 Prompt 的片段段落；降级时为空字符串。
    pub prompt_section: String,
    /// 注入后的完整 Prompt；未提供 base_prompt 时为 `null`。
    pub final_prompt: Option<String>,
    /// `prompt_section` 的估算 token 数。
    pub token_estimate: usize,
    /// 画像来源。
    pub profile_source: ProfileSource,
}

/// 登记片段请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterSnippetRequest {
    /// 可选主键；缺省由服务端生成。
    #[serde(default)]
    pub id: Option<String>,
    /// 来源类型。
    pub source_type: SnippetSource,
    /// 来源引用（生成任务 ID / 模板版本）。
    #[serde(default)]
    pub source_ref: Option<String>,
    /// 场景编码。
    pub scene_code: String,
    /// 标题。
    pub title: String,
    /// 片段 HTML。
    pub snippet_html: String,
    /// 检索用纯文本；缺省从 HTML 提取。
    #[serde(default)]
    pub snippet_text: Option<String>,
    /// 标签。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 风格特征。
    #[serde(default)]
    pub style: SnippetStyle,
    /// 显式满意度；新片段缺省 0.5，更新已有片段时保留原值。
    #[serde(default)]
    pub satisfaction: Option<f64>,
    /// 隐式满意度；缺省由反馈计数推导。
    #[serde(default)]
    pub implicit_score: Option<f64>,
}

impl RegisterSnippetRequest {
    /// 校验请求；返回规范化后的字段错误信息。
    pub fn validate(&self) -> Result<(), String> {
        if let Some(id) = &self.id {
            validate_identifier(id, "id", 64)?;
        }
        if let Some(source_ref) = &self.source_ref {
            validate_text(source_ref, "source_ref", 128)?;
        }
        validate_identifier(&self.scene_code, "scene_code", SCENE_CODE_MAX_CHARS)?;
        validate_text(&self.title, "title", TITLE_MAX_CHARS)?;
        validate_html(&self.snippet_html)?;
        if let Some(text) = &self.snippet_text {
            validate_text(text, "snippet_text", SNIPPET_TEXT_MAX_CHARS)?;
        }
        if self.tags.len() > TAG_MAX_COUNT {
            return Err(format!("tags 最多 {TAG_MAX_COUNT} 个"));
        }
        for tag in &self.tags {
            validate_text(tag, "tags[]", TAG_MAX_CHARS)?;
        }
        validate_style(&self.style)?;
        if let Some(satisfaction) = self.satisfaction {
            validate_unit("satisfaction", satisfaction)?;
        }
        if let Some(score) = self.implicit_score {
            validate_unit("implicit_score", score)?;
        }
        Ok(())
    }
}

/// 片段反馈请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnippetFeedbackRequest {
    /// 本次满意度（0.0 ~ 1.0）。
    pub satisfaction: f64,
}

impl SnippetFeedbackRequest {
    /// 校验请求。
    pub fn validate(&self) -> Result<(), String> {
        validate_unit("satisfaction", self.satisfaction)
    }
}

/// 检索请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrieveRequest {
    /// 可选用户标识；显式提供时必须与鉴权用户一致。
    #[serde(default)]
    pub user_id: Option<String>,
    /// 场景编码。
    pub scene_code: String,
    /// 语义查询文本。
    #[serde(default)]
    pub query: Option<String>,
    /// 覆盖默认 top_k。
    #[serde(default)]
    pub top_k: Option<usize>,
    /// 覆盖默认 token 预算。
    #[serde(default)]
    pub token_budget: Option<usize>,
    /// 基础 Prompt；提供时返回注入后的完整 Prompt。
    #[serde(default)]
    pub base_prompt: Option<String>,
}

impl RetrieveRequest {
    /// 校验请求。
    pub fn validate(&self) -> Result<(), String> {
        validate_identifier(&self.scene_code, "scene_code", SCENE_CODE_MAX_CHARS)?;
        if let Some(query) = &self.query {
            if !query.trim().is_empty() {
                validate_text(query, "query", QUERY_MAX_CHARS)?;
            }
        }
        if let Some(base_prompt) = &self.base_prompt {
            if !base_prompt.trim().is_empty() {
                validate_text(base_prompt, "base_prompt", BASE_PROMPT_MAX_CHARS)?;
            }
        }
        if let Some(top_k) = self.top_k {
            if !(1..=10).contains(&top_k) {
                return Err("top_k 必须在 1..=10 之间".to_string());
            }
        }
        if let Some(budget) = self.token_budget {
            if !(64..=8_000).contains(&budget) {
                return Err("token_budget 必须在 64..=8000 之间".to_string());
            }
        }
        Ok(())
    }

    /// 转为领域查询。
    pub fn into_query(self) -> FewShotQuery {
        FewShotQuery {
            scene_code: self.scene_code,
            query: self.query,
            top_k: self.top_k,
            token_budget: self.token_budget,
            base_prompt: self.base_prompt,
        }
    }
}

/// 列表查询参数。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSnippetsQuery {
    /// 可选场景过滤。
    #[serde(default)]
    pub scene_code: Option<String>,
    /// 返回条数，默认 20，最大 100。
    #[serde(default)]
    pub limit: Option<usize>,
    /// 偏移量。
    #[serde(default)]
    pub offset: Option<usize>,
}

impl ListSnippetsQuery {
    /// 校验参数。
    pub fn validate(&self) -> Result<(), String> {
        if let Some(scene_code) = &self.scene_code {
            validate_identifier(scene_code, "scene_code", SCENE_CODE_MAX_CHARS)?;
        }
        if let Some(limit) = self.limit {
            if !(1..=100).contains(&limit) {
                return Err("limit 必须在 1..=100 之间".to_string());
            }
        }
        Ok(())
    }

    /// 规范化分页。
    pub fn pagination(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(20).clamp(1, 100) as i64;
        let offset = self.offset.unwrap_or(0) as i64;
        (limit, offset)
    }
}

/// 标识符校验：ASCII 字母、数字与 `._:-`。
pub fn validate_identifier(value: &str, field: &str, max_chars: usize) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{field} 不能为空"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{field} 最长 {max_chars} 个字符"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'))
    {
        return Err(format!("{field} 仅允许 ASCII 字母、数字与 . _ : -"));
    }
    Ok(())
}

/// 普通文本校验：非空、长度受限、禁止控制字符。
pub fn validate_text(value: &str, field: &str, max_chars: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} 不能为空"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{field} 最长 {max_chars} 个字符"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t' | '\r'))
    {
        return Err(format!("{field} 含非法控制字符"));
    }
    Ok(())
}

/// 片段 HTML 校验：长度、控制字符与危险标记。
pub fn validate_html(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("snippet_html 不能为空".to_string());
    }
    if value.chars().count() > SNIPPET_HTML_MAX_CHARS {
        return Err(format!("snippet_html 最长 {SNIPPET_HTML_MAX_CHARS} 个字符"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t' | '\r'))
    {
        return Err("snippet_html 含非法控制字符".to_string());
    }
    let lower = value.to_ascii_lowercase();
    for pattern in [
        "<script",
        "</script",
        "<iframe",
        "<object",
        "<embed",
        "javascript:",
        "vbscript:",
        "data:text/html",
    ] {
        if lower.contains(pattern) {
            return Err(format!("snippet_html 不允许包含 `{pattern}`"));
        }
    }
    if has_event_handler(&lower) {
        return Err("snippet_html 不允许包含内联事件处理器（on*=）".to_string());
    }
    Ok(())
}

/// 检测 `on...=` 形式的内联事件处理器。
fn has_event_handler(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    for index in 0..bytes.len().saturating_sub(2) {
        if bytes[index] != b'o' || bytes[index + 1] != b'n' {
            continue;
        }
        // 属性名边界：行首或前一个字符不是字母 / 数字，覆盖空格、换行与 `<`。
        if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
            continue;
        }
        let mut end = index + 2;
        while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
            end += 1;
        }
        if end == index + 2 {
            continue;
        }
        let mut equals = end;
        while equals < bytes.len() && bytes[equals].is_ascii_whitespace() {
            equals += 1;
        }
        if equals < bytes.len() && bytes[equals] == b'=' {
            return true;
        }
    }
    false
}

/// 风格字段范围校验。
pub fn validate_style(style: &SnippetStyle) -> Result<(), String> {
    if let Some(value) = style.font_scale {
        validate_range("style.font_scale", value, 0.8, 1.4)?;
    }
    if let Some(value) = style.density {
        validate_range("style.density", value, 0.0, 1.0)?;
    }
    if let Some(value) = style.radius {
        validate_range("style.radius", value, 0.0, 24.0)?;
    }
    if let Some(value) = style.spacing {
        validate_range("style.spacing", value, 0.75, 1.5)?;
    }
    Ok(())
}

fn validate_unit(field: &str, value: f64) -> Result<(), String> {
    validate_range(field, value, 0.0, 1.0)
}

fn validate_range(field: &str, value: f64, min: f64, max: f64) -> Result<(), String> {
    if !value.is_finite() || value < min || value > max {
        return Err(format!("{field} 必须在 {min}..={max} 之间"));
    }
    Ok(())
}

/// 从 HTML 提取用于检索的纯文本。
pub fn derive_snippet_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }

    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");

    collapse_whitespace(&decoded)
        .chars()
        .take(SNIPPET_TEXT_MAX_CHARS)
        .collect()
}

fn collapse_whitespace(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut last_was_space = true;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }
    }
    result.trim().to_string()
}

/// 按字符截断（不会切坏 UTF-8）。
pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_plain_text_from_html() {
        let text = derive_snippet_text("<div>订单 <strong>详情</strong>&nbsp;卡片</div>");
        assert!(text.contains("订单"));
        assert!(text.contains("详情"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn rejects_script_and_event_handlers() {
        assert!(validate_html("<div onclick=\"x()\">a</div>").is_err());
        assert!(validate_html("<script>alert(1)</script>").is_err());
        assert!(validate_html("<div class=\"card\">ok</div>").is_ok());
    }

    #[test]
    fn identifier_charset_is_enforced() {
        assert!(validate_identifier("ORDER_DETAIL_PAGE", "scene", 64).is_ok());
        assert!(validate_identifier("订单", "scene", 64).is_err());
        assert!(validate_identifier("", "scene", 64).is_err());
    }

    #[test]
    fn quality_score_combines_explicit_and_implicit() {
        let snippet = ComponentSnippet {
            id: "FSP_1".to_string(),
            source_type: SnippetSource::Curated,
            source_ref: None,
            scene_code: "SCENE".to_string(),
            title: "t".to_string(),
            snippet_html: "<div></div>".to_string(),
            snippet_text: "t".to_string(),
            tags: vec![],
            style: SnippetStyle::default(),
            satisfaction: 1.0,
            implicit_score: 1.0,
            usage_count: 0,
            positive_feedback: 0,
            negative_feedback: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert!(snippet.quality_score() > 0.9);
    }
}
