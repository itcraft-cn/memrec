//! # MMR 多样性重排
//!
//! 实现 Maximal Marginal Relevance 算法，在相关性与多样性之间取得平衡。

use std::collections::HashSet;

/// MMR 配置。
#[derive(Debug, Clone)]
pub struct MmrConfig {
    /// 相关性权重（0.0-1.0），默认 0.5
    pub lambda: f64,
    /// 返回数量
    pub top_k: usize,
    /// 候选池大小
    pub max_candidates: usize,
}

impl Default for MmrConfig {
    fn default() -> Self {
        Self {
            lambda: 0.5,
            top_k: 10,
            max_candidates: 50,
        }
    }
}

/// MMR 重排。
///
/// 公式：MMR(d) = λ × rel(d) - (1-λ) × max(sim(d, d')) for d' in S
pub fn mmr_rerank<T: Clone + AsRef<str>>(
    candidates: Vec<T>,
    scores: Vec<f64>,
    config: &MmrConfig,
) -> Vec<T> {
    // TODO: 实现
    candidates.into_iter().take(config.top_k).collect()
}

fn tokenize(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Hello World!");
        assert!(tokens.contains("hello"));
        assert!(tokens.contains("world"));
    }

    #[test]
    fn test_jaccard_identical() {
        let a = tokenize("hello world");
        let b = tokenize("hello world");
        assert!((jaccard(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_disjoint() {
        let a = tokenize("hello");
        let b = tokenize("world");
        assert!((jaccard(&a, &b) - 0.0).abs() < 0.001);
    }
}
