//! 轻量文本检索工具：中英文分词、余弦相似度与 token 估算。
//!
//! 不引入外部 embedding 依赖：RAG 的第一阶段以「场景过滤 + 词法相似度 +
//! 画像亲和度」为主，部署零依赖、延迟可控。中文使用「单字 + 相邻二元组」，
//! 英文 / 数字按词切分，兼顾召回与精确匹配。

use std::collections::HashMap;

/// 判断字符是否属于 CJK 统一表意文字（含扩展 A 与常用兼容区）。
pub fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
    )
}

/// 文本分词：CJK 单字 + 相邻二元组，ASCII 按词。
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cjk_run: Vec<char> = Vec::new();
    let mut ascii_word = String::new();

    let flush_ascii = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(word.to_ascii_lowercase());
            word.clear();
        }
    };

    let flush_cjk = |run: &mut Vec<char>, tokens: &mut Vec<String>| {
        for ch in run.iter() {
            tokens.push(ch.to_string());
        }
        for pair in run.windows(2) {
            tokens.push(format!("{}{}", pair[0], pair[1]));
        }
        run.clear();
    };

    for ch in text.chars() {
        if is_cjk(ch) {
            flush_ascii(&mut ascii_word, &mut tokens);
            cjk_run.push(ch);
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            flush_cjk(&mut cjk_run, &mut tokens);
            ascii_word.push(ch);
        } else {
            flush_ascii(&mut ascii_word, &mut tokens);
            flush_cjk(&mut cjk_run, &mut tokens);
        }
    }
    flush_ascii(&mut ascii_word, &mut tokens);
    flush_cjk(&mut cjk_run, &mut tokens);
    tokens
}

/// 余弦相似度（词频向量），返回 0.0 ~ 1.0。
pub fn cosine_similarity(left: &[String], right: &[String]) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let mut left_counts: HashMap<&str, f64> = HashMap::new();
    for token in left {
        *left_counts.entry(token.as_str()).or_insert(0.0) += 1.0;
    }
    let mut right_counts: HashMap<&str, f64> = HashMap::new();
    for token in right {
        *right_counts.entry(token.as_str()).or_insert(0.0) += 1.0;
    }

    let mut dot = 0.0;
    for (token, left_count) in &left_counts {
        if let Some(right_count) = right_counts.get(token) {
            dot += left_count * right_count;
        }
    }
    if dot == 0.0 {
        return 0.0;
    }

    let left_norm = left_counts.values().map(|v| v * v).sum::<f64>().sqrt();
    let right_norm = right_counts.values().map(|v| v * v).sum::<f64>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    (dot / (left_norm * right_norm)).clamp(0.0, 1.0)
}

/// 估算文本 token 数。
///
/// 规则偏保守（宁多勿少）：CJK 字符按 1 token/字，其余字符按 4 字符/token。
pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if is_cjk(ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_chinese_into_unigrams_and_bigrams() {
        let tokens = tokenize("订单详情");
        assert!(tokens.contains(&"订".to_string()));
        assert!(tokens.contains(&"订单".to_string()));
        assert!(tokens.contains(&"详情".to_string()));
    }

    #[test]
    fn identical_text_has_similarity_one() {
        let left = tokenize("订单详情 卡片");
        let right = tokenize("订单详情 卡片");
        assert!((cosine_similarity(&left, &right) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unrelated_text_has_zero_similarity() {
        let left = tokenize("订单详情");
        let right = tokenize("product price");
        assert_eq!(cosine_similarity(&left, &right), 0.0);
    }

    #[test]
    fn token_estimate_is_conservative_for_chinese() {
        assert_eq!(estimate_tokens("订单详情"), 4);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }
}
