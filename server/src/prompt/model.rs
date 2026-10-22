//! 动态 System Prompt 拼装器的领域模型（REQ-202610-1182）。
//!
//! 输入由四部分组成：用户画像（REQ-202610-1176）、场景约束（REQ-202610-1171）、
//! 业务数据与 Few-Shot 片段（REQ-202610-1183）。输出为版本化、稳定可复现的
//! System Prompt 及分段结构，便于测试与审计。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 模板标识（与具体场景无关）。
pub const PROMPT_TEMPLATE_ID: &str = "intimeui.system_prompt";
/// 当前模板版本；模板文本或分段规则变更时必须递增。
pub const PROMPT_TEMPLATE_VERSION: &str = "v1";
/// 业务数据序列化后的最大字节数（与 UI 生成契约一致）。
pub const MAX_BUSINESS_DATA_BYTES: usize = 64 * 1024;
/// 单次拼装允许的最大 Few-Shot 条数。
pub const MAX_FEW_SHOTS: usize = 8;
/// 单条 Few-Shot 输入的最大字符数。
pub const MAX_FEW_SHOT_INPUT_CHARS: usize = 2_000;
/// 单条 Few-Shot 输出的最大字节数。
pub const MAX_FEW_SHOT_OUTPUT_BYTES: usize = 32 * 1024;
/// 场景约束列表的最大条数。
pub const MAX_SCENE_RULES: usize = 32;
/// 单条场景约束的最大字符数。
pub const MAX_SCENE_RULE_CHARS: usize = 200;
/// 敏感字段替换值。
pub const REDACTED_VALUE: &str = "[REDACTED]";

/// 场景约束（来自 REQ-202610-1171 的场景注册表，由调用方传入）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneConstraints {
    /// 场景编码，如 `ORDER_DETAIL_PAGE`。
    pub scene_code: String,
    /// 场景展示名称。
    pub display_name: String,
    /// 页面必须包含的元素。
    #[serde(default)]
    pub required_elements: Vec<String>,
    /// 允许的交互动作。
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    /// 明确禁止的内容或行为。
    #[serde(default)]
    pub forbidden: Vec<String>,
    /// 布局 / 视觉补充说明。
    #[serde(default)]
    pub layout_notes: Vec<String>,
}

impl SceneConstraints {
    /// 校验场景约束的长度、字符集与列表上限。
    pub fn validate(&self) -> Result<(), PromptError> {
        validate_identifier("scene_code", &self.scene_code, 64)?;
        validate_text("display_name", &self.display_name, 128)?;
        validate_rules("required_elements", &self.required_elements)?;
        validate_rules("allowed_actions", &self.allowed_actions)?;
        validate_rules("forbidden", &self.forbidden)?;
        validate_rules("layout_notes", &self.layout_notes)?;
        Ok(())
    }
}

/// Few-Shot 示例（来自 REQ-202610-1183 的模板检索）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FewShotExample {
    /// 示例对应的用户需求。
    pub input: String,
    /// 期望的单文件 HTML 输出。
    pub output: String,
    /// 可选场景编码，用于检索结果标注。
    #[serde(default)]
    pub scene_code: Option<String>,
}

impl FewShotExample {
    /// 校验单条示例；保留标签由拼装器独占，示例中不得出现。
    pub fn validate(&self) -> Result<(), PromptError> {
        validate_text("few_shot.input", &self.input, MAX_FEW_SHOT_INPUT_CHARS)?;
        if self.output.trim().is_empty() {
            return Err(PromptError::InvalidInput(
                "few_shot.output 不能为空".to_string(),
            ));
        }
        if self.output.len() > MAX_FEW_SHOT_OUTPUT_BYTES {
            return Err(PromptError::TooLarge(format!(
                "few_shot.output 超过 {MAX_FEW_SHOT_OUTPUT_BYTES} 字节"
            )));
        }
        if let Some(scene_code) = &self.scene_code {
            validate_identifier("few_shot.scene_code", scene_code, 64)?;
        }
        let combined = format!("{}\n{}", self.input, self.output).to_ascii_lowercase();
        for reserved in [
            "<few_shot",
            "</few_shot",
            "<business_data",
            "</business_data",
        ] {
            if combined.contains(reserved) {
                return Err(PromptError::InvalidInput(format!(
                    "few_shot 内容不得包含保留标签 `{reserved}`"
                )));
            }
        }
        Ok(())
    }
}

/// Prompt 分段类型：指令与数据严格分离，便于审计与注入测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionKind {
    /// 可信指令。
    Instruction,
    /// 不可信数据（仅作为模型输入，不作为指令）。
    Data,
}

/// 一个 Prompt 分段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptSection {
    /// 分段键名，稳定且唯一。
    pub key: String,
    /// 分段类型。
    pub kind: PromptSectionKind,
    /// 分段标题（用于调试与审计）。
    pub title: String,
    /// 分段正文。
    pub content: String,
}

/// 拼装结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssembledPrompt {
    /// 模板标识。
    pub template_id: &'static str,
    /// 模板版本。
    pub template_version: &'static str,
    /// 完整 System Prompt。
    pub system_prompt: String,
    /// 稳定指纹（FNV-1a 64，非加密用途）。
    pub prompt_hash: String,
    /// 分段结构（指令在前、数据在后）。
    pub sections: Vec<PromptSection>,
    /// 被脱敏的字段路径（已排序）。
    pub filtered_fields: Vec<String>,
    /// 检测到的疑似指令注入片段（仅告警，不改变指令语义）。
    pub injection_warnings: Vec<String>,
}

impl AssembledPrompt {
    /// 按 key 查找分段。
    pub fn section(&self, key: &str) -> Option<&PromptSection> {
        self.sections.iter().find(|section| section.key == key)
    }
}

/// 拼装错误。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PromptError {
    /// 输入不合法。
    #[error("invalid prompt input: {0}")]
    InvalidInput(String),
    /// 输入超过大小限制。
    #[error("prompt input too large: {0}")]
    TooLarge(String),
}

pub(crate) fn validate_identifier(
    field: &str,
    value: &str,
    max_chars: usize,
) -> Result<(), PromptError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max_chars {
        return Err(PromptError::InvalidInput(format!(
            "{field} 长度必须在 1..={max_chars} 之间"
        )));
    }
    if trimmed != value {
        return Err(PromptError::InvalidInput(format!(
            "{field} 不得包含首尾空白"
        )));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'))
    {
        return Err(PromptError::InvalidInput(format!(
            "{field} 仅允许 ASCII 字母、数字、`.`、`_`、`:`、`-`"
        )));
    }
    Ok(())
}

pub(crate) fn validate_text(field: &str, value: &str, max_chars: usize) -> Result<(), PromptError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max_chars {
        return Err(PromptError::InvalidInput(format!(
            "{field} 长度必须在 1..={max_chars} 之间"
        )));
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(PromptError::InvalidInput(format!(
            "{field} 不得包含控制字符"
        )));
    }
    Ok(())
}

fn validate_rules(field: &str, rules: &[String]) -> Result<(), PromptError> {
    if rules.len() > MAX_SCENE_RULES {
        return Err(PromptError::InvalidInput(format!(
            "{field} 最多 {MAX_SCENE_RULES} 条"
        )));
    }
    for rule in rules {
        validate_text(field, rule, MAX_SCENE_RULE_CHARS)?;
    }
    Ok(())
}
