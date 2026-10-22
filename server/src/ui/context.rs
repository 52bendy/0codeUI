//! UI 渲染上下文与场景渲染错误。
//!
//! 场景注册表（[`crate::ui::scene`]）只依赖这里的抽象类型，
//! 模板实现（[`crate::ui::template`]）也只接收 [`SceneContext`]，
//! 从而把「场景发现」与「具体渲染」解耦。

use std::fmt;

use serde_json::Value;

/// 场景渲染策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStrategy {
    /// 静态模板渲染（第一阶段全部场景使用）。
    Template,
    /// 大模型动态生成（后续阶段接入，见 REQ-202610-1185）。
    Dynamic,
}

impl RenderStrategy {
    /// 用于 `version` 字段的策略标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Template => "template",
            Self::Dynamic => "dynamic",
        }
    }
}

impl fmt::Display for RenderStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一次渲染所需的全部上下文。
#[derive(Debug)]
pub struct SceneContext<'a> {
    /// 场景编码（与注册表一致）。
    pub scene_code: &'a str,
    /// 设备屏幕宽度（CSS px）。
    pub screen_width: u32,
    /// 设备屏幕高度（CSS px）。
    pub screen_height: u32,
    /// 归一化后的设备平台：android / ios / web / harmonyos。
    pub platform: &'a str,
    /// 调用方传入的业务数据（已通过场景契约校验）。
    pub business_data: &'a Value,
    /// 完整版本串：`<strategy>:<scene_code>:<template_version>`。
    pub version: &'a str,
    /// 模板版本（如 `v1`）。
    pub template_version: &'a str,
}

/// 场景渲染错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    /// 业务数据不符合场景契约（映射为 HTTP 400 / code 40000）。
    InvalidBusinessData(String),
    /// 渲染过程内部错误（映射为 HTTP 500 / code 50000）。
    Render(String),
}

impl SceneError {
    /// 业务数据不合法。
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidBusinessData(message.into())
    }

    /// 渲染内部错误。
    pub fn render(message: impl Into<String>) -> Self {
        Self::Render(message.into())
    }
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBusinessData(message) => write!(f, "invalid business_data: {message}"),
            Self::Render(message) => write!(f, "render failed: {message}"),
        }
    }
}

impl std::error::Error for SceneError {}
