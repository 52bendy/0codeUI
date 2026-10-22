//! 模型调用领域类型（与 OpenAI Chat Completions 兼容）。

use serde::{Deserialize, Serialize};

/// 对话角色。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// 系统提示。
    System,
    /// 用户输入。
    User,
    /// 助手输出。
    Assistant,
}

/// 单条对话消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// 角色。
    pub role: Role,
    /// 文本内容。
    pub content: String,
}

impl ChatMessage {
    /// 系统消息。
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    /// 用户消息。
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
}

/// 一次推理请求。
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// 模型名。
    pub model: String,
    /// 消息列表。
    pub messages: Vec<ChatMessage>,
    /// 最大输出 token。
    pub max_tokens: u32,
    /// 采样温度。
    pub temperature: f32,
    /// 是否流式。
    pub stream: bool,
}

/// token 用量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// 输入 token 数。
    #[serde(default)]
    pub prompt_tokens: u32,
    /// 输出 token 数。
    #[serde(default)]
    pub completion_tokens: u32,
    /// 总 token 数。
    #[serde(default)]
    pub total_tokens: u32,
}

impl Usage {
    /// 合并增量用量（流式场景）。
    pub fn merge(&mut self, other: Usage) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
    }
}

/// 非流式推理结果。
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// 文本内容。
    pub content: String,
    /// 结束原因。
    pub finish_reason: Option<String>,
    /// token 用量。
    pub usage: Usage,
}

/// 流式分片。
#[derive(Debug, Clone, Default)]
pub struct StreamChunk {
    /// 增量文本。
    pub delta: Option<String>,
    /// 结束原因（最后一个内容分片）。
    pub finish_reason: Option<String>,
    /// 用量（通常出现在最终分片）。
    pub usage: Option<Usage>,
}
