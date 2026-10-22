//! AI 生成管线编排与降级兜底（REQ-202610-1185）。
//!
//! 链路：场景校验 → 画像加载 → Prompt 拼装（REQ-202610-1182）→ 模型网关
//! （REQ-202610-1181）→ 单文件 HTML 校验（REQ-202610-1173）→ 输出混淆
//! （REQ-202610-1184）→ 返回。任一步失败或超时，立即切换静态模板
//! （REQ-202610-1172），保证首屏有页面、无白屏。
//!
//! 入口：[`Pipeline::generate`]；HTTP 端点见 `routes::pipeline`。

pub mod cache;
pub mod config;
pub mod metrics;
pub mod obfuscation;
pub mod orchestrator;

pub use config::{ObfuscationMode, PipelineConfig};
pub use metrics::{PipelineMetrics, PipelineMetricsSnapshot};
pub use obfuscation::{HtmlObfuscator, NoopObfuscator, ObfuscationOutput};
pub use orchestrator::{
    dynamic_version, DegradationReason, Pipeline, PipelineError, PipelineInfo, PipelineOutcome,
    PipelineRequest, PipelineTimings,
};
