//! UI 生成模型接入与推理服务化（REQ-202610-1181）。
//!
//! 组成：
//! - [`config`]：`MODEL_*` 环境变量与边界校验；
//! - [`provider`]：推理提供者抽象（OpenAI 兼容 / mock）；
//! - [`gateway`]：模型路由、灰度、并发排队、超时重试、指标与输出校验；
//! - [`prompt`]：UI 生成系统提示词与单文件 HTML 提取；
//! - [`metrics`]：请求量、延迟分位、token 与成本估算。

pub mod config;
pub mod error;
pub mod gateway;
pub mod metrics;
pub mod mock;
pub mod openai;
pub mod prompt;
pub mod provider;
pub mod types;

pub use config::{ModelConfig, ProviderKind};
pub use error::ModelError;
pub use gateway::{GatewayInfo, ModelGateway, StreamEvent, UiGeneration, UiRequest, UiStream};
pub use metrics::{MetricsSnapshot, ModelMetrics};
pub use provider::{ModelProvider, ModelStream};
pub use types::{ChatMessage, ChatRequest, ChatResponse, Role, StreamChunk, Usage};
