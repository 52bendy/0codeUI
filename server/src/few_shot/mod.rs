//! Few-Shot 模版检索（RAG）模块（REQ-202610-1183）。
//!
//! 模块结构：
//! - [`model`]：片段、配置、请求 / 响应与输入校验；
//! - [`text`]：中英文分词、余弦相似度与 token 估算；
//! - [`rank`]：语义相似度 + 画像亲和度 + 质量分的加权排序；
//! - [`prompt`]：Prompt 片段渲染、代码围栏与 token 预算；
//! - [`service`]：画像 / 信号加载、检索编排与安全降级。
//!
//! 对外入口是 [`FewShotService::retrieve`]：14 Prompt 拼装器只需把基础 Prompt
//! 传入即可拿到注入后的完整 Prompt；未接入拼装器时也可直接调用 HTTP 接口验收。

pub mod model;
pub mod prompt;
pub mod rank;
pub mod service;
pub mod text;

pub use model::{
    ComponentSnippet, DegradeReason, FewShotConfig, FewShotQuery, FewShotResult, ListSnippetsQuery,
    ProfileSource, RegisterSnippetRequest, RetrieveRequest, RetrievedSnippet,
    SnippetFeedbackRequest, SnippetSource, SnippetStyle, SnippetTheme,
};
pub use prompt::{append_to_prompt, render_prompt_section, PromptRender};
pub use rank::{rank_candidates, ScoredSnippet};
pub use service::{FewShotError, FewShotService};
