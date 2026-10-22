//! 推理提供者抽象：屏蔽 vLLM / Ollama / 外部 OpenAI 兼容服务的差异。

use async_trait::async_trait;
use futures_util::stream::BoxStream;

use super::error::ModelError;
use super::types::{ChatRequest, ChatResponse, StreamChunk};

/// 模型流式输出（`'static` 便于直接交给 Axum SSE）。
pub type ModelStream = BoxStream<'static, Result<StreamChunk, ModelError>>;

/// 推理提供者。
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// 提供者名称（用于日志与指标）。
    fn name(&self) -> &'static str;

    /// 非流式生成。
    async fn generate(&self, request: ChatRequest) -> Result<ChatResponse, ModelError>;

    /// 建立流式生成；返回的流在消费时产生增量分片。
    async fn generate_stream(&self, request: ChatRequest) -> Result<ModelStream, ModelError>;
}
