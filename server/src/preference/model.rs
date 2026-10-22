//! 偏好对领域模型（REQ-202610-1188，PRD §4.2）。
//!
//! 一条偏好对表示同一 prompt 下的两个候选 HTML：`chosen_html` 优于
//! `rejected_html`。除训练字段外，模型显式保存溯源信息（用户 / 场景 /
//! prompt 模板版本 / 模型版本 / 管线版本）与清洗结果（质量分 / 质量标记）。

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

/// prompt 最大字符数的默认上限。
pub const MAX_PROMPT_CHARS: usize = 8_000;
/// 单段 HTML 最大字节数的默认上限（256 KiB）。
pub const MAX_HTML_BYTES: usize = 262_144;
/// 元数据 JSON 最大字节数。
pub const MAX_METADATA_BYTES: usize = 8_192;
/// 标识符最大长度。
pub const MAX_IDENTIFIER_LEN: usize = 128;
/// 审核备注最大长度。
pub const MAX_REVIEW_NOTE_CHARS: usize = 500;

/// 偏好对来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PreferenceSource {
    /// 显式偏好调研（REQ-202610-1175，数据源 07）。
    ExplicitSurvey,
    /// 隐式行为指标（停留时长、点击、重新生成等）。
    ImplicitSignal,
    /// A/B 对比实验。
    AbTest,
    /// 人工标注 / 人工修正。
    Manual,
}

impl PreferenceSource {
    /// 全部来源。
    pub const ALL: [Self; 4] = [
        Self::ExplicitSurvey,
        Self::ImplicitSignal,
        Self::AbTest,
        Self::Manual,
    ];

    /// 数据库 / API 字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitSurvey => "EXPLICIT_SURVEY",
            Self::ImplicitSignal => "IMPLICIT_SIGNAL",
            Self::AbTest => "AB_TEST",
            Self::Manual => "MANUAL",
        }
    }

    /// 从数据库字符串解析。
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == value)
    }
}

/// 偏好对状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PairStatus {
    /// 待人工审核（质量中等或存在非阻断标记）。
    Pending,
    /// 已通过，可进入导出。
    Accepted,
    /// 已拒绝，不进入数据集。
    Rejected,
    /// 已导出到训练数据集。
    Exported,
}

impl PairStatus {
    /// 全部状态。
    pub const ALL: [Self; 4] = [
        Self::Pending,
        Self::Accepted,
        Self::Rejected,
        Self::Exported,
    ];

    /// 数据库 / API 字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Accepted => "ACCEPTED",
            Self::Rejected => "REJECTED",
            Self::Exported => "EXPORTED",
        }
    }

    /// 从数据库字符串解析。
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

/// 审核决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    /// 通过。
    Accept,
    /// 拒绝。
    Reject,
}

impl ReviewDecision {
    /// 对应的目标状态。
    pub const fn target_status(self) -> PairStatus {
        match self {
            Self::Accept => PairStatus::Accepted,
            Self::Reject => PairStatus::Rejected,
        }
    }
}

/// 已落库的偏好对。
#[derive(Debug, Clone, Serialize)]
pub struct PreferencePair {
    /// 服务端主键（`PREF_<uuid>`）。
    pub id: String,
    /// 幂等键：`sha256(user_id | source | scene | content_hash)`。
    pub pair_key: String,
    /// 内容哈希：`sha256(prompt | chosen | rejected)`，用于跨用户去重。
    pub content_hash: String,
    /// 用户 prompt。
    pub prompt: String,
    /// 优选 HTML。
    pub chosen_html: String,
    /// 劣选 HTML。
    pub rejected_html: String,
    /// 来源（`PreferenceSource::as_str`）。
    pub source: String,
    /// 场景编码。
    pub scene_code: String,
    /// 用户标识（仅库内保存，导出时哈希化）。
    pub user_id: String,
    /// 会话标识。
    pub session_id: Option<String>,
    /// 请求 / 链路标识。
    pub request_id: Option<String>,
    /// 关联的调研幂等键（显式调研来源）。
    pub survey_id: Option<String>,
    /// Prompt 模板版本。
    pub prompt_template_version: String,
    /// chosen 候选的模型版本。
    pub chosen_model: Option<String>,
    /// rejected 候选的模型版本。
    pub rejected_model: Option<String>,
    /// chosen 候选的样式标识。
    pub chosen_style_id: Option<String>,
    /// rejected 候选的样式标识。
    pub rejected_style_id: Option<String>,
    /// 生成管线版本。
    pub pipeline_version: Option<String>,
    /// 偏好强度（0–1）。
    pub preference_strength: f64,
    /// 清洗质量分（0–1）。
    pub quality_score: f64,
    /// 质量标记（如 `pii_redacted` / `rejected_html_invalid`）。
    pub quality_flags: Vec<String>,
    /// 扩展元数据（已脱敏）。
    pub metadata: Value,
    /// 状态（`PairStatus::as_str`）。
    pub status: String,
    /// 审核备注。
    pub review_note: Option<String>,
    /// 审核人。
    pub reviewed_by: Option<String>,
    /// 审核时间。
    pub reviewed_at: Option<DateTime<Utc>>,
    /// 导出时间。
    pub exported_at: Option<DateTime<Utc>>,
    /// 偏好发生时间。
    pub occurred_at: DateTime<Utc>,
    /// 入库时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 通用入库请求（隐式指标 / A/B 实验 / 人工标注均走此入口）。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct PairInput {
    /// 用户 prompt。
    #[validate(length(min = 1, max = 8000, message = "prompt must be 1..=8000 characters"))]
    pub prompt: String,
    /// 优选 HTML。
    pub chosen_html: String,
    /// 劣选 HTML。
    pub rejected_html: String,
    /// 来源。
    pub source: PreferenceSource,
    /// 场景编码。
    #[validate(length(min = 1, max = 128, message = "scene_code must be 1..=128 characters"))]
    pub scene_code: String,
    /// 会话标识。
    #[validate(length(min = 1, max = 128, message = "session_id must be 1..=128 characters"))]
    pub session_id: Option<String>,
    /// 请求 / 链路标识。
    #[validate(length(min = 1, max = 128, message = "request_id must be 1..=128 characters"))]
    pub request_id: Option<String>,
    /// Prompt 模板版本。
    #[validate(length(
        min = 1,
        max = 64,
        message = "prompt_template_version must be 1..=64 characters"
    ))]
    pub prompt_template_version: Option<String>,
    /// chosen 候选模型版本。
    #[validate(length(
        min = 1,
        max = 128,
        message = "chosen_model must be 1..=128 characters"
    ))]
    pub chosen_model: Option<String>,
    /// rejected 候选模型版本。
    #[validate(length(
        min = 1,
        max = 128,
        message = "rejected_model must be 1..=128 characters"
    ))]
    pub rejected_model: Option<String>,
    /// chosen 候选样式标识。
    #[validate(length(
        min = 1,
        max = 128,
        message = "chosen_style_id must be 1..=128 characters"
    ))]
    pub chosen_style_id: Option<String>,
    /// rejected 候选样式标识。
    #[validate(length(
        min = 1,
        max = 128,
        message = "rejected_style_id must be 1..=128 characters"
    ))]
    pub rejected_style_id: Option<String>,
    /// 生成管线版本。
    #[validate(length(
        min = 1,
        max = 64,
        message = "pipeline_version must be 1..=64 characters"
    ))]
    pub pipeline_version: Option<String>,
    /// 偏好强度（0–1，缺省 1.0）。
    #[validate(range(min = 0.0, max = 1.0, message = "must be in [0, 1]"))]
    pub preference_strength: Option<f64>,
    /// 扩展元数据（仅接受 JSON 对象）。
    pub metadata: Option<Value>,
    /// 偏好发生时间（RFC3339）；缺省为服务端接收时间。
    pub occurred_at: Option<DateTime<Utc>>,
}

/// 由显式调研生成偏好对的请求。
///
/// 调研（REQ-202610-1175）只记录样式选择，不含 HTML；调用方需同时提供
/// 两个候选样式对应的 HTML。场景 / 会话 / 样式 / 用户均从调研记录读取，
/// 避免客户端伪造溯源信息。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct FromSurveyInput {
    /// 调研幂等键。
    #[validate(length(min = 1, max = 64, message = "survey_id must be 1..=64 characters"))]
    pub survey_id: String,
    /// 用户 prompt。
    #[validate(length(min = 1, max = 8000, message = "prompt must be 1..=8000 characters"))]
    pub prompt: String,
    /// 被选择样式的 HTML。
    pub chosen_html: String,
    /// 被拒绝样式的 HTML。
    pub rejected_html: String,
    /// Prompt 模板版本。
    #[validate(length(
        min = 1,
        max = 64,
        message = "prompt_template_version must be 1..=64 characters"
    ))]
    pub prompt_template_version: Option<String>,
    /// chosen 候选模型版本。
    #[validate(length(
        min = 1,
        max = 128,
        message = "chosen_model must be 1..=128 characters"
    ))]
    pub chosen_model: Option<String>,
    /// rejected 候选模型版本。
    #[validate(length(
        min = 1,
        max = 128,
        message = "rejected_model must be 1..=128 characters"
    ))]
    pub rejected_model: Option<String>,
    /// 生成管线版本。
    #[validate(length(
        min = 1,
        max = 64,
        message = "pipeline_version must be 1..=64 characters"
    ))]
    pub pipeline_version: Option<String>,
    /// 偏好强度（0–1，缺省 1.0）。
    #[validate(range(min = 0.0, max = 1.0, message = "must be in [0, 1]"))]
    pub preference_strength: Option<f64>,
    /// 扩展元数据（仅接受 JSON 对象）。
    pub metadata: Option<Value>,
}

/// 内部草稿：路由层把请求转换为与来源无关的结构。
#[derive(Debug, Clone)]
pub struct PairDraft {
    /// 用户 prompt。
    pub prompt: String,
    /// 优选 HTML。
    pub chosen_html: String,
    /// 劣选 HTML。
    pub rejected_html: String,
    /// 来源。
    pub source: String,
    /// 场景编码。
    pub scene_code: String,
    /// 会话标识。
    pub session_id: Option<String>,
    /// 请求标识。
    pub request_id: Option<String>,
    /// 调研幂等键。
    pub survey_id: Option<String>,
    /// Prompt 模板版本。
    pub prompt_template_version: String,
    /// chosen 模型版本。
    pub chosen_model: Option<String>,
    /// rejected 模型版本。
    pub rejected_model: Option<String>,
    /// chosen 样式标识。
    pub chosen_style_id: Option<String>,
    /// rejected 样式标识。
    pub rejected_style_id: Option<String>,
    /// 管线版本。
    pub pipeline_version: Option<String>,
    /// 偏好强度。
    pub preference_strength: f64,
    /// 元数据。
    pub metadata: Value,
    /// 偏好发生时间。
    pub occurred_at: Option<DateTime<Utc>>,
}

impl PairInput {
    /// 转换为内部草稿。
    pub fn into_draft(self) -> PairDraft {
        PairDraft {
            prompt: self.prompt,
            chosen_html: self.chosen_html,
            rejected_html: self.rejected_html,
            source: self.source.as_str().to_string(),
            scene_code: self.scene_code,
            session_id: self.session_id,
            request_id: self.request_id,
            survey_id: None,
            prompt_template_version: self
                .prompt_template_version
                .unwrap_or_else(|| "unknown".to_string()),
            chosen_model: self.chosen_model,
            rejected_model: self.rejected_model,
            chosen_style_id: self.chosen_style_id,
            rejected_style_id: self.rejected_style_id,
            pipeline_version: self.pipeline_version,
            preference_strength: self.preference_strength.unwrap_or(1.0),
            metadata: self
                .metadata
                .unwrap_or_else(|| Value::Object(Default::default())),
            occurred_at: self.occurred_at,
        }
    }
}

/// 入库结果（幂等：重复内容返回已有记录）。
#[derive(Debug, Clone, Serialize)]
pub struct IngestOutcome {
    /// 是否新写入。
    pub accepted: bool,
    /// 是否命中幂等键并返回已有记录。
    pub duplicate: bool,
    /// 落库记录。
    pub pair: PreferencePair,
}

/// 偏好对过滤条件。
#[derive(Debug, Clone, Default)]
pub struct PairFilter {
    /// 状态集合（空表示不过滤）。
    pub statuses: Vec<String>,
    /// 来源。
    pub source: Option<String>,
    /// 场景编码。
    pub scene_code: Option<String>,
    /// 用户标识。
    pub user_id: Option<String>,
    /// 最低质量分。
    pub min_quality: Option<f64>,
    /// 起始入库时间（含）。
    pub from: Option<DateTime<Utc>>,
    /// 结束入库时间（含）。
    pub to: Option<DateTime<Utc>>,
}

/// 分页结果。
#[derive(Debug, Clone, Serialize)]
pub struct PairPage {
    /// 当前页记录。
    pub items: Vec<PreferencePair>,
    /// 总条数。
    pub total: i64,
    /// 页大小。
    pub limit: i64,
    /// 偏移。
    pub offset: i64,
}

/// 单个聚合桶。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CountBucket {
    /// 维度值。
    pub key: String,
    /// 数量。
    pub count: i64,
}

/// 偏好数据统计。
#[derive(Debug, Clone, Serialize)]
pub struct PreferenceStats {
    /// 总记录数。
    pub total: i64,
    /// 按状态分布。
    pub by_status: Vec<CountBucket>,
    /// 按来源分布。
    pub by_source: Vec<CountBucket>,
    /// 按场景分布（Top 20）。
    pub by_scene: Vec<CountBucket>,
    /// 平均质量分。
    pub average_quality: f64,
    /// 可导出记录数（ACCEPTED）。
    pub exportable: i64,
    /// 自动拒绝的低质量记录数。
    pub low_quality_rejected: i64,
    /// 最近入库时间。
    pub last_created_at: Option<DateTime<Utc>>,
}

/// 抽样结果（管理端检查用，保留原始 user_id 便于溯源）。
#[derive(Debug, Clone, Serialize)]
pub struct SampleResult {
    /// 抽样种子。
    pub seed: String,
    /// 请求条数。
    pub limit: usize,
    /// 候选总数。
    pub candidate_count: usize,
    /// 抽样记录。
    pub items: Vec<PreferencePair>,
}

/// 审核请求。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct ReviewInput {
    /// 审核决定。
    pub decision: ReviewDecision,
    /// 审核备注。
    #[validate(length(max = 500, message = "note must be at most 500 characters"))]
    pub note: Option<String>,
}

/// 导出请求。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct ExportInput {
    /// 最大记录数。
    #[validate(range(min = 1, max = 5000, message = "must be in 1..=5000"))]
    pub limit: Option<usize>,
    /// 最低质量分。
    #[validate(range(min = 0.0, max = 1.0, message = "must be in [0, 1]"))]
    pub min_quality: Option<f64>,
    /// 单用户最大记录数。
    #[validate(range(min = 1, max = 10000, message = "must be in 1..=10000"))]
    pub max_per_user: Option<usize>,
    /// 单场景最大记录数。
    #[validate(range(min = 1, max = 100000, message = "must be in 1..=100000"))]
    pub max_per_scene: Option<usize>,
    /// 场景过滤。
    #[validate(length(min = 1, max = 128, message = "scene_code must be 1..=128 characters"))]
    pub scene_code: Option<String>,
    /// 来源过滤。
    pub source: Option<PreferenceSource>,
    /// 是否包含待审核记录（默认 false，仅导出 ACCEPTED）。
    #[serde(default)]
    pub include_pending: bool,
    /// 试运行：不写导出记录、不改状态。
    #[serde(default)]
    pub dry_run: bool,
}

/// 导出选项（已填充默认值）。
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// 最大记录数。
    pub limit: usize,
    /// 最低质量分。
    pub min_quality: f64,
    /// 单用户最大记录数。
    pub max_per_user: usize,
    /// 单场景最大记录数。
    pub max_per_scene: usize,
    /// 场景过滤。
    pub scene_code: Option<String>,
    /// 来源过滤。
    pub source: Option<String>,
    /// 是否包含待审核记录。
    pub include_pending: bool,
    /// 试运行。
    pub dry_run: bool,
}

/// 导出记录（DPO 训练格式）。
#[derive(Debug, Clone, Serialize)]
pub struct DpoRecord {
    /// 偏好对 ID。
    pub id: String,
    /// prompt。
    pub prompt: String,
    /// 优选输出。
    pub chosen: String,
    /// 劣选输出。
    pub rejected: String,
    /// 溯源元数据（用户已哈希化）。
    pub metadata: DpoMetadata,
}

/// 导出记录的溯源元数据。
#[derive(Debug, Clone, Serialize)]
pub struct DpoMetadata {
    /// 来源。
    pub source: String,
    /// 场景编码。
    pub scene_code: String,
    /// 用户哈希（`u_<sha256(salt:user_id)[..16]>`）。
    pub user_hash: String,
    /// 会话哈希（存在时）。
    pub session_hash: Option<String>,
    /// 请求标识。
    pub request_id: Option<String>,
    /// 调研幂等键。
    pub survey_id: Option<String>,
    /// Prompt 模板版本。
    pub prompt_template_version: String,
    /// chosen 模型版本。
    pub chosen_model: Option<String>,
    /// rejected 模型版本。
    pub rejected_model: Option<String>,
    /// chosen 样式标识。
    pub chosen_style_id: Option<String>,
    /// rejected 样式标识。
    pub rejected_style_id: Option<String>,
    /// 管线版本。
    pub pipeline_version: Option<String>,
    /// 偏好强度。
    pub preference_strength: f64,
    /// 质量分。
    pub quality_score: f64,
    /// 质量标记。
    pub quality_flags: Vec<String>,
    /// 偏好发生时间。
    pub occurred_at: DateTime<Utc>,
    /// 入库时间。
    pub created_at: DateTime<Utc>,
}

/// 导出跳过原因统计。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExportSkipStats {
    /// 内容重复跳过数。
    pub duplicate_content: usize,
    /// 单用户配额跳过数。
    pub user_quota: usize,
    /// 单场景配额跳过数。
    pub scene_quota: usize,
    /// 字节预算跳过数。
    pub byte_budget: usize,
}

/// 导出清单。
#[derive(Debug, Clone, Serialize)]
pub struct ExportManifest {
    /// 数据集 ID（`DS_<uuid>`）。
    pub dataset_id: String,
    /// 数据格式。
    pub format: String,
    /// 生成时间。
    pub created_at: DateTime<Utc>,
    /// 记录数。
    pub record_count: usize,
    /// 按来源统计。
    pub source_counts: BTreeMap<String, usize>,
    /// 按场景统计。
    pub scene_counts: BTreeMap<String, usize>,
    /// 去重后的用户数。
    pub unique_user_count: usize,
    /// 最低质量分。
    pub min_quality: f64,
    /// 单用户配额。
    pub max_per_user: usize,
    /// 单场景配额。
    pub max_per_scene: usize,
    /// JSONL 内容 SHA-256。
    pub sha256: String,
    /// 盐指纹（用于核对导出是否使用同一盐，不暴露盐本身）。
    pub user_hash_fingerprint: String,
    /// 跳过统计。
    pub skipped: ExportSkipStats,
    /// 过滤条件回显。
    pub filters: Value,
}

/// 导出结果。
#[derive(Debug, Clone, Serialize)]
pub struct ExportBundle {
    /// 清单。
    pub manifest: ExportManifest,
    /// 记录。
    pub records: Vec<DpoRecord>,
}

impl ExportBundle {
    /// 渲染为 JSONL（每条记录一行，末尾带换行）。
    pub fn to_jsonl(&self) -> Result<String, serde_json::Error> {
        let mut output = String::new();
        for record in &self.records {
            output.push_str(&serde_json::to_string(record)?);
            output.push('\n');
        }
        Ok(output)
    }
}

/// 导出批次记录（`preference_exports` 表）。
#[derive(Debug, Clone)]
pub struct PreferenceExportRecord {
    /// 主键（`EXP_<uuid>`）。
    pub id: String,
    /// 数据集 ID。
    pub dataset_id: String,
    /// 数据格式。
    pub format: String,
    /// 记录数。
    pub record_count: i64,
    /// JSONL SHA-256。
    pub sha256: String,
    /// 过滤条件 JSON。
    pub filters_json: String,
    /// 完整清单 JSON。
    pub manifest_json: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 偏好数据管道错误。
#[derive(Debug, thiserror::Error)]
pub enum PreferenceError {
    /// 入参非法。
    #[error("invalid preference pair: {0}")]
    Invalid(String),
    /// 资源不存在。
    #[error("{0}")]
    NotFound(String),
    /// 状态冲突。
    #[error("{0}")]
    Conflict(String),
    /// 服务内部错误。
    #[error("internal error: {0}")]
    Internal(String),
    /// 仓储错误。
    #[error(transparent)]
    Repository(#[from] crate::repository::RepositoryError),
}

impl From<PreferenceError> for crate::error::AppError {
    fn from(error: PreferenceError) -> Self {
        match error {
            PreferenceError::Invalid(message) => crate::error::AppError::invalid_param(message),
            PreferenceError::NotFound(message) => {
                crate::error::AppError::new(crate::error::ErrorCode::NotFound, message)
            }
            PreferenceError::Conflict(message) => {
                crate::error::AppError::new(crate::error::ErrorCode::Conflict, message)
            }
            PreferenceError::Internal(message) => crate::error::AppError::internal(message),
            PreferenceError::Repository(error) => error.into(),
        }
    }
}
