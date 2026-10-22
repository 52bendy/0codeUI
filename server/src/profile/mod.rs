//! 用户画像引擎（REQ-202610-1176）。
//!
//! 模块结构：
//! - [`model`]：偏好向量、画像、Prompt 注入结构等领域模型；
//! - [`signals`]：1174 埋点 / 1175 调研 → 标准观测的映射；
//! - [`engine`]：EWMA 计算、单步限幅、死区与更新频率控制。
//!
//! 对外入口通常只需 [`ProfileEngine`] 与 [`ProfileEngineConfig`]。

pub mod engine;
pub mod model;
pub mod signals;

pub use engine::{
    confidence_for, ProfileEngine, ProfileEngineConfig, UpdateOutcome, UpdateSkipReason,
};
pub use model::{
    ConfidenceLevel, PreferenceVector, ProfileResponse, ProfileUpdate, PromptProfile,
    PublicPreferences, Theme, UserProfile, DENSITY_MAX, DENSITY_MIN, FONT_SCALE_MAX,
    FONT_SCALE_MIN, RADIUS_MAX, RADIUS_MIN, SPACING_MAX, SPACING_MIN, THEME_DARK_THRESHOLD,
};
pub use signals::{
    Observation, ObservationSource, PartialPreferenceVector, SurveySample, TelemetrySample,
};
