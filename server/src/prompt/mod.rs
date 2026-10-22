//! 动态 System Prompt 拼装器（REQ-202610-1182）。
//!
//! 将用户画像、业务数据、场景约束与 Few-Shot 片段拼装为版本化、稳定可复现
//! 的 System Prompt。核心入口为 [`PromptAssembler::assemble`]。
//!
//! 安全设计：
//! - 画像与场景约束是可信指令；业务数据与 Few-Shot 是隔离的数据区块；
//! - 业务数据递归脱敏，并以稳定 JSON 序列化后转义 `<` / `>` / `&`，
//!   无法提前闭合 `<business_data>`；
//! - 数据中的指令式文本只产生告警，不会进入指令分段。

mod assembler;
pub mod model;
mod sanitize;
pub mod scenes;

pub use assembler::PromptAssembler;
pub use model::{
    AssembledPrompt, FewShotExample, PromptError, PromptSection, PromptSectionKind,
    SceneConstraints, MAX_BUSINESS_DATA_BYTES, MAX_FEW_SHOTS, PROMPT_TEMPLATE_ID,
    PROMPT_TEMPLATE_VERSION, REDACTED_VALUE,
};
pub use scenes::{builtin_scene_constraints, BUILTIN_SCENE_CODES};
