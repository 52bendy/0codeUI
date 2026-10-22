//! 偏好对齐数据管道（REQ-202610-1188，PRD §4.2）。
//!
//! 目标：把显式调研（REQ-202610-1175）、隐式行为指标与 A/B 实验统一归集为
//! `<Prompt, Chosen_HTML, Rejected_HTML>` 偏好对，完成清洗 / 脱敏 / 去偏后
//! 导出 DPO 训练数据，并保留完整溯源（用户 / 场景 / 模板版本 / 模型版本）。
//!
//! 模块划分：
//! - [`config`]：`PREFERENCE_*` 环境变量；
//! - [`model`]：领域模型、请求 / 响应结构与错误类型；
//! - [`cleaning`]：归一化、PII 脱敏、HTML 校验、质量打分；
//! - [`service`]：入库 / 调研归集 / 审核 / 抽样 / 导出编排。
//!
//! 训练脚本骨架与「训练 → 评测 → 灰度」流程见 `training/dpo/` 与
//! `docs/dpo-rollout-process.md`。

pub mod cleaning;
pub mod config;
pub mod model;
pub mod service;

pub use config::PreferenceConfig;
pub use model::{
    DpoRecord, ExportBundle, ExportManifest, IngestOutcome, PairFilter, PairInput, PairPage,
    PairStatus, PreferencePair, PreferenceSource, PreferenceStats, ReviewDecision, SampleResult,
};
pub use service::PreferenceService;
