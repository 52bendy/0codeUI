//! 输出混淆阶段的可插拔接口。
//!
//! 真正的混淆实现由 REQ-202610-1184 的 `obfuscation` 模块提供；管线只依赖
//! [`HtmlObfuscator`] 抽象，便于测试注入、按配置关闭，以及在依赖分支未合入时
//! 保持管线可编译。混淆失败被视为管线失败并触发静态模板兜底。

use std::sync::Arc;

use crate::obfuscation::{ObfuscationConfig, ObfuscationEngine};

/// 混淆结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObfuscationOutput {
    /// 输出 HTML。
    pub html: String,
    /// 是否实际执行了混淆。
    pub applied: bool,
}

/// 输出混淆器。
pub trait HtmlObfuscator: Send + Sync {
    /// 混淆一段完整 HTML；`session_id` 用于同一会话内的稳定类名映射。
    fn obfuscate(&self, html: &str, session_id: Option<&str>) -> Result<ObfuscationOutput, String>;
}

/// 关闭混淆时的直通实现。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopObfuscator;

impl HtmlObfuscator for NoopObfuscator {
    fn obfuscate(
        &self,
        html: &str,
        _session_id: Option<&str>,
    ) -> Result<ObfuscationOutput, String> {
        Ok(ObfuscationOutput {
            html: html.to_string(),
            applied: false,
        })
    }
}

/// 基于 REQ-202610-1184 [`ObfuscationEngine`] 的适配器。
pub struct EngineObfuscator {
    engine: Arc<ObfuscationEngine>,
    config: ObfuscationConfig,
}

impl EngineObfuscator {
    /// 使用共享引擎与固定配置创建适配器。
    pub fn new(engine: Arc<ObfuscationEngine>, config: ObfuscationConfig) -> Self {
        Self { engine, config }
    }
}

impl HtmlObfuscator for EngineObfuscator {
    fn obfuscate(&self, html: &str, session_id: Option<&str>) -> Result<ObfuscationOutput, String> {
        let result = self
            .engine
            .obfuscate(html, &self.config, session_id)
            .map_err(|error| error.to_string())?;
        let applied = self.config.level.enabled();
        Ok(ObfuscationOutput {
            html: result.html,
            applied,
        })
    }
}
