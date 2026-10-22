//! 动态 UI 生成（M1 第一阶段：模板兜底）。
//!
//! 模块边界：
//! - [`device`]：设备信息与平台归一化；
//! - [`context`]：渲染上下文与场景错误；
//! - [`scene`]：场景注册表（scene_code → 策略 / 配置 / 渲染器）；
//! - [`template`]：单文件 HTML 模板实现；
//! - [`generate`]：请求 / 响应契约与生成用例。
//!
//! 输出约束见 [`template`]：完整 HTML、内联 CSS/JS、无外部资源、全字段转义。

pub mod context;
pub mod device;
pub mod generate;
pub mod scene;
pub mod template;

pub use context::{RenderStrategy, SceneContext, SceneError};
pub use device::DeviceInfo;
pub use generate::{generate_ui, RenderType, UiGenerateData, UiGenerateRequest};
pub use scene::{SceneConfig, SceneDefinition, SceneRegistry, REGISTRY};
