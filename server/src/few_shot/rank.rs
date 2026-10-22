//! 片段排序：语义相似度、画像亲和度与质量分的加权组合。
//!
//! 排序是确定性的：同分时按更新时间倒序、ID 升序，保证同一输入可复现，
//! 便于人工抽检与回归测试。

use crate::profile::PreferenceVector;

use super::model::{ComponentSnippet, SnippetStyle};
use super::text::cosine_similarity;

/// 语义相似度权重。
pub const SEMANTIC_WEIGHT: f64 = 0.50;
/// 画像亲和度权重。
pub const PROFILE_WEIGHT: f64 = 0.30;
/// 质量分权重。
pub const QUALITY_WEIGHT: f64 = 0.20;

/// 带打分的候选片段。
#[derive(Debug, Clone)]
pub struct ScoredSnippet {
    /// 原始片段。
    pub snippet: ComponentSnippet,
    /// 综合得分。
    pub score: f64,
    /// 语义相似度。
    pub semantic_score: f64,
    /// 画像亲和度。
    pub profile_score: f64,
    /// 质量分（含场景参与度修正）。
    pub quality_score: f64,
}

/// 对候选片段排序。
///
/// - `query_tokens`：查询文本分词结果；为空时语义分取中性值 0.5；
/// - `profile`：用户偏好向量；`None` 表示冷启动，画像分取中性值 0.5；
/// - `engagement`：该用户在该场景的隐式参与度（0.0 ~ 1.0），用于微调质量分。
pub fn rank_candidates(
    candidates: &[ComponentSnippet],
    query_tokens: &[String],
    profile: Option<&PreferenceVector>,
    engagement: Option<f64>,
) -> Vec<ScoredSnippet> {
    let mut scored: Vec<ScoredSnippet> = candidates
        .iter()
        .map(|snippet| {
            let semantic = if query_tokens.is_empty() {
                0.5
            } else {
                cosine_similarity(query_tokens, &snippet_tokens(snippet))
            };
            let profile_score = style_affinity(&snippet.style, profile);
            let quality = quality_with_engagement(snippet, engagement);
            let score = SEMANTIC_WEIGHT * semantic
                + PROFILE_WEIGHT * profile_score
                + QUALITY_WEIGHT * quality;

            ScoredSnippet {
                snippet: snippet.clone(),
                score: score.clamp(0.0, 1.0),
                semantic_score: semantic,
                profile_score,
                quality_score: quality,
            }
        })
        .collect();

    scored.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.snippet.updated_at.cmp(&left.snippet.updated_at))
            .then_with(|| left.snippet.id.cmp(&right.snippet.id))
    });
    scored
}

/// 片段检索文本 = 标题 + 标签 + 纯文本。
fn snippet_tokens(snippet: &ComponentSnippet) -> Vec<String> {
    let mut text = String::new();
    text.push_str(&snippet.title);
    text.push(' ');
    text.push_str(&snippet.tags.join(" "));
    text.push(' ');
    text.push_str(&snippet.snippet_text);
    super::text::tokenize(&text)
}

/// 片段风格与用户画像的亲和度（0.0 ~ 1.0）。
pub fn style_affinity(style: &SnippetStyle, profile: Option<&PreferenceVector>) -> f64 {
    let Some(profile) = profile else {
        return 0.5;
    };

    let mut dimensions: Vec<f64> = Vec::new();
    dimensions.push(style.theme.match_score(profile.theme()));

    if let Some(value) = style.font_scale {
        dimensions.push(normalized_affinity(value, profile.font_scale, 0.6));
    }
    if let Some(value) = style.density {
        dimensions.push(normalized_affinity(value, profile.density, 1.0));
    }
    if let Some(value) = style.radius {
        dimensions.push(normalized_affinity(value, profile.radius, 24.0));
    }
    if let Some(value) = style.spacing {
        dimensions.push(normalized_affinity(value, profile.spacing, 0.75));
    }

    if dimensions.is_empty() {
        return 0.5;
    }
    dimensions.iter().sum::<f64>() / dimensions.len() as f64
}

fn normalized_affinity(value: f64, target: f64, range: f64) -> f64 {
    if !value.is_finite() || !target.is_finite() || range <= 0.0 {
        return 0.5;
    }
    (1.0 - (value - target).abs() / range).clamp(0.0, 1.0)
}

fn quality_with_engagement(snippet: &ComponentSnippet, engagement: Option<f64>) -> f64 {
    let base = snippet.quality_score();
    match engagement {
        Some(value) if value.is_finite() => {
            (base * (0.85 + 0.30 * value.clamp(0.0, 1.0))).clamp(0.0, 1.0)
        }
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::few_shot::model::{SnippetSource, SnippetTheme};

    fn snippet(id: &str, theme: SnippetTheme, density: f64) -> ComponentSnippet {
        let now = Utc.with_ymd_and_hms(2026, 10, 22, 0, 0, 0).unwrap();
        ComponentSnippet {
            id: id.to_string(),
            source_type: SnippetSource::Curated,
            source_ref: None,
            scene_code: "ORDER_DETAIL_PAGE".to_string(),
            title: "订单详情卡片".to_string(),
            snippet_html: "<div>订单详情</div>".to_string(),
            snippet_text: "订单详情 状态 金额".to_string(),
            tags: vec!["order".to_string()],
            style: SnippetStyle {
                theme,
                font_scale: Some(1.0),
                density: Some(density),
                radius: Some(8.0),
                spacing: Some(1.0),
            },
            satisfaction: 0.8,
            implicit_score: 0.8,
            usage_count: 0,
            positive_feedback: 0,
            negative_feedback: 0,
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn dark_profile_prefers_dark_snippet() {
        let profile = PreferenceVector {
            theme_score: 1.0,
            density: 0.2,
            ..PreferenceVector::default()
        };
        let dark = snippet("FSP_DARK", SnippetTheme::Dark, 0.2);
        let light = snippet("FSP_LIGHT", SnippetTheme::Light, 0.9);
        let ranked = rank_candidates(&[light, dark], &[], Some(&profile), None);
        assert_eq!(ranked[0].snippet.id, "FSP_DARK");
    }

    #[test]
    fn semantic_match_beats_unrelated_text() {
        let related = snippet("FSP_A", SnippetTheme::Any, 0.5);
        let mut unrelated = snippet("FSP_B", SnippetTheme::Any, 0.5);
        unrelated.title = "商品价格".to_string();
        unrelated.snippet_text = "商品 价格 库存".to_string();
        unrelated.tags = vec!["product".to_string()];
        let tokens = super::super::text::tokenize("订单详情 金额");
        let ranked = rank_candidates(&[unrelated, related], &tokens, None, None);
        assert_eq!(ranked[0].snippet.id, "FSP_A");
    }
}
