//! 行为信号 → 偏好目标的映射。
//!
//! 两个上游需求分别产生不同形态的数据：
//! - REQ-202610-1174 埋点事件：停留时长、点击率、误触率、滚动深度；
//! - REQ-202610-1175 偏好调研：用户选择的风格与拒绝的风格（style_id）。
//!
//! 本模块把它们统一成 [`Observation`]：一个带权重、带时间戳的
//! [`PartialPreferenceVector`]。缺失的维度保持 `None`，不参与 EWMA 计算。

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::model::{
    PreferenceVector, DENSITY_MAX, DENSITY_MIN, FONT_SCALE_MAX, FONT_SCALE_MIN, RADIUS_MAX,
    RADIUS_MIN, SPACING_MAX, SPACING_MIN,
};

/// 行为信号来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    /// 埋点行为事件（1174）。
    Telemetry,
    /// 偏好调研（1175）。
    Survey,
}

impl ObservationSource {
    /// 来源权重：调研是用户显式选择，可信度高于隐式行为。
    pub fn weight_factor(self) -> f64 {
        match self {
            Self::Telemetry => 0.75,
            Self::Survey => 1.25,
        }
    }
}

/// 部分偏好向量：只包含本次信号能推断出的维度。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartialPreferenceVector {
    /// 字号缩放目标。
    pub font_scale: Option<f64>,
    /// 密度目标。
    pub density: Option<f64>,
    /// 主题连续分数目标。
    pub theme_score: Option<f64>,
    /// 圆角目标（px）。
    pub radius: Option<f64>,
    /// 间距缩放目标。
    pub spacing: Option<f64>,
}

impl PartialPreferenceVector {
    /// 是否没有提供任何维度。
    pub fn is_empty(&self) -> bool {
        self.font_scale.is_none()
            && self.density.is_none()
            && self.theme_score.is_none()
            && self.radius.is_none()
            && self.spacing.is_none()
    }
}

/// 一条标准化后的行为观测。
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// 推断出的偏好目标。
    pub vector: PartialPreferenceVector,
    /// 观测权重（0.0 ~ 1.0，调用方可进一步降权）。
    pub weight: f64,
    /// 信号来源。
    pub source: ObservationSource,
    /// 信号发生时间。
    pub observed_at: DateTime<Utc>,
}

impl Observation {
    /// 由埋点事件构造观测。
    ///
    /// 映射规则（全部先做区间收敛，避免异常值直接污染画像）：
    /// - 字号：参与度越高、滚动越深，字号略增；
    /// - 密度：参与度越高，密度越低（更紧凑，单屏信息量更大）；
    /// - 主题 / 圆角 / 间距：隐式行为无法可靠推断，保持 `None`。
    pub fn from_telemetry(
        dwell_time_ms: i64,
        element_click_rate: f64,
        misclick_rate: f64,
        scroll_depth: f64,
        observed_at: DateTime<Utc>,
    ) -> Self {
        let dwell = (dwell_time_ms.max(0) as f64 / 120_000.0).clamp(0.0, 1.0);
        let click = sanitize_rate(element_click_rate);
        let misclick = sanitize_rate(misclick_rate);
        let scroll = sanitize_rate(scroll_depth);

        let engagement = (click - misclick).clamp(-1.0, 1.0);
        let dwell_centered = (dwell - 0.5) * 2.0;
        let scroll_centered = (scroll - 0.5) * 2.0;

        let font_scale = (1.0 + 0.08 * engagement + 0.04 * scroll_centered + 0.03 * dwell_centered)
            .clamp(FONT_SCALE_MIN, FONT_SCALE_MAX);
        let density = (0.5 - 0.18 * engagement - 0.08 * scroll_centered + 0.04 * dwell_centered)
            .clamp(DENSITY_MIN, DENSITY_MAX);

        Self {
            vector: PartialPreferenceVector {
                font_scale: Some(font_scale),
                density: Some(density),
                theme_score: None,
                radius: None,
                spacing: None,
            },
            weight: 0.75,
            source: ObservationSource::Telemetry,
            observed_at,
        }
    }

    /// 由偏好调研构造观测。
    ///
    /// 选中的风格直接映射为目标；选中风格未覆盖的维度，用「拒绝风格的反向值」补齐。
    /// 单次调研的权重固定为 1.0，是否足以翻转主题由引擎的限幅与 EWMA 决定。
    pub fn from_survey(
        selected_style_id: &str,
        rejected_style_id: &str,
        observed_at: DateTime<Utc>,
    ) -> Self {
        let selected = parse_style_id(selected_style_id);
        let rejected = parse_style_id(rejected_style_id);

        let vector = PartialPreferenceVector {
            font_scale: selected
                .font_scale
                .or_else(|| rejected.font_scale.map(invert_font)),
            density: selected
                .density
                .or_else(|| rejected.density.map(invert_density)),
            theme_score: selected
                .theme_score
                .or_else(|| rejected.theme_score.map(invert_theme)),
            radius: selected
                .radius
                .or_else(|| rejected.radius.map(invert_radius)),
            spacing: selected
                .spacing
                .or_else(|| rejected.spacing.map(invert_spacing)),
        };

        Self {
            vector,
            weight: 1.0,
            source: ObservationSource::Survey,
            observed_at,
        }
    }
}

/// 1174 埋点事件的领域快照（仓储层查询结果）。
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetrySample {
    /// 停留时长（毫秒）。
    pub dwell_time_ms: i64,
    /// 元素点击率。
    pub element_click_rate: f64,
    /// 误触率。
    pub misclick_rate: f64,
    /// 滚动深度。
    pub scroll_depth: f64,
    /// 事件发生时间。
    pub occurred_at: DateTime<Utc>,
}

impl TelemetrySample {
    /// 转为标准观测。
    pub fn into_observation(self) -> Observation {
        Observation::from_telemetry(
            self.dwell_time_ms,
            self.element_click_rate,
            self.misclick_rate,
            self.scroll_depth,
            self.occurred_at,
        )
    }
}

/// 1175 偏好调研的领域快照（仓储层查询结果）。
#[derive(Debug, Clone, PartialEq)]
pub struct SurveySample {
    /// 选中的风格 ID。
    pub selected_style_id: String,
    /// 拒绝的风格 ID。
    pub rejected_style_id: String,
    /// 调研发生时间。
    pub occurred_at: DateTime<Utc>,
}

impl SurveySample {
    /// 转为标准观测。
    pub fn into_observation(self) -> Observation {
        Observation::from_survey(
            &self.selected_style_id,
            &self.rejected_style_id,
            self.occurred_at,
        )
    }
}

/// 把当前画像与观测目标合并为一条可直接展示的偏好向量（调试用）。
pub fn merge_targets(current: &PreferenceVector, observation: &Observation) -> PreferenceVector {
    PreferenceVector {
        font_scale: observation.vector.font_scale.unwrap_or(current.font_scale),
        density: observation.vector.density.unwrap_or(current.density),
        theme_score: observation
            .vector
            .theme_score
            .unwrap_or(current.theme_score),
        radius: observation.vector.radius.unwrap_or(current.radius),
        spacing: observation.vector.spacing.unwrap_or(current.spacing),
    }
}

fn sanitize_rate(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// 解析风格 ID 中的语义 token。
///
/// 约定风格 ID 使用 `style_<theme>_<density>_<font>_<radius>_<spacing>` 形式，
/// 例如 `style_dark_compact_large_font_rounded_wide_spacing`。
fn parse_style_id(style_id: &str) -> PartialPreferenceVector {
    let normalized = style_id.trim().to_ascii_lowercase();

    let theme_score = if normalized.contains("dark") {
        Some(1.0)
    } else if normalized.contains("light") {
        Some(0.0)
    } else {
        None
    };

    let density = if normalized.contains("compact") || normalized.contains("dense") {
        Some(0.15)
    } else if normalized.contains("spacious") || normalized.contains("comfortable") {
        Some(0.85)
    } else {
        None
    };

    let font_scale = if normalized.contains("large_font") || normalized.contains("large-font") {
        Some(1.2)
    } else if normalized.contains("small_font") || normalized.contains("small-font") {
        Some(0.9)
    } else if normalized.contains("medium_font") || normalized.contains("medium-font") {
        Some(1.0)
    } else {
        None
    };

    let radius = if normalized.contains("pill") {
        Some(24.0)
    } else if normalized.contains("rounded") {
        Some(16.0)
    } else if normalized.contains("sharp") {
        Some(4.0)
    } else {
        None
    };

    let spacing = if normalized.contains("wide_spacing") || normalized.contains("wide-spacing") {
        Some(1.2)
    } else if normalized.contains("tight_spacing") || normalized.contains("tight-spacing") {
        Some(0.85)
    } else if normalized.contains("spacious_spacing") {
        Some(1.25)
    } else {
        None
    };

    PartialPreferenceVector {
        font_scale,
        density,
        theme_score,
        radius,
        spacing,
    }
}

fn invert_theme(value: f64) -> f64 {
    (1.0 - value).clamp(0.0, 1.0)
}

fn invert_density(value: f64) -> f64 {
    (1.0 - value).clamp(DENSITY_MIN, DENSITY_MAX)
}

fn invert_font(value: f64) -> f64 {
    (2.0 - value).clamp(FONT_SCALE_MIN, FONT_SCALE_MAX)
}

fn invert_radius(value: f64) -> f64 {
    (20.0 - value).clamp(RADIUS_MIN, RADIUS_MAX)
}

fn invert_spacing(value: f64) -> f64 {
    (2.0 - value).clamp(SPACING_MIN, SPACING_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 22, 8, 0, 0).unwrap()
    }

    #[test]
    fn telemetry_mapping_clamps_abnormal_input() {
        let observation = Observation::from_telemetry(i64::MAX, 99.0, -5.0, f64::NAN, now());
        let font = observation.vector.font_scale.unwrap();
        let density = observation.vector.density.unwrap();
        assert!((FONT_SCALE_MIN..=FONT_SCALE_MAX).contains(&font));
        assert!((DENSITY_MIN..=DENSITY_MAX).contains(&density));
        assert_eq!(observation.weight, 0.75);
    }

    #[test]
    fn rejected_style_fills_missing_dimensions() {
        let observation = Observation::from_survey("style_dark", "style_light_spacious", now());
        assert_eq!(observation.vector.theme_score, Some(1.0));
        assert!((observation.vector.density.unwrap() - 0.15).abs() < 1e-9);
    }
}
