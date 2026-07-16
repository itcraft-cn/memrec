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

/// MMR 搜索命中项接口。
pub trait MmrHit: Clone {
    /// 获取分数。
    fn score(&self) -> f64;
    /// 获取文本内容用于相似度计算。
    fn text(&self) -> &str;
}

/// MMR 重排。
///
/// 公式：MMR(d) = λ × rel(d) - (1-λ) × max(sim(d, d')) for d' in S
pub fn mmr_rerank<H: MmrHit>(candidates: Vec<H>, config: &MmrConfig) -> Vec<H> {
    if candidates.is_empty() || config.top_k == 0 {
        return Vec::new();
    }

    let limit = config.top_k.min(candidates.len());
    let mut selected: Vec<H> = Vec::with_capacity(limit);
    let mut remaining: Vec<H> = candidates;

    let mut token_sets: Vec<Option<HashSet<String>>> = remaining
        .iter()
        .map(|h| Some(tokenize(h.text())))
        .collect();

    while selected.len() < limit && !remaining.is_empty() {
        let mut best_idx = 0;
        let mut best_mmr = f64::MIN;

        for (i, candidate) in remaining.iter().enumerate() {
            let relevance = candidate.score();

            let max_sim = selected
                .iter()
                .map(|s| {
                    let s_tokens = tokenize(s.text());
                    if let Some(ref c_tokens) = token_sets[i] {
                        jaccard(c_tokens, &s_tokens)
                    } else {
                        0.0
                    }
                })
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0);

            let mmr = config.lambda * relevance - (1.0 - config.lambda) * max_sim;

            if mmr > best_mmr {
                best_mmr = mmr;
                best_idx = i;
            }
        }

        let hit = remaining.remove(best_idx);
        token_sets.remove(best_idx);
        selected.push(hit);
    }

    selected
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

    #[derive(Clone)]
    struct TestHit {
        text: String,
        score: f64,
    }

    impl MmrHit for TestHit {
        fn score(&self) -> f64 {
            self.score
        }
        fn text(&self) -> &str {
            &self.text
        }
    }

    #[test]
    fn test_mmr_rerank_diversity() {
        let candidates = vec![
            TestHit {
                text: "hello world".to_string(),
                score: 0.9,
            },
            TestHit {
                text: "hello world test".to_string(),
                score: 0.85,
            },
            TestHit {
                text: "foo bar".to_string(),
                score: 0.8,
            },
        ];

        let config = MmrConfig {
            lambda: 0.5,
            top_k: 2,
            max_candidates: 50,
        };
        let result = mmr_rerank(candidates, &config);

        assert_eq!(result.len(), 2);
        assert!((result[0].score() - 0.9).abs() < 0.001);
        assert!(result[1].text().contains("foo"));
    }

    #[test]
    fn test_mmr_rerank_empty() {
        let candidates: Vec<TestHit> = vec![];
        let config = MmrConfig::default();
        let result = mmr_rerank(candidates, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_mmr_rerank_single() {
        let candidates = vec![TestHit {
            text: "single".to_string(),
            score: 0.9,
        }];
        let config = MmrConfig {
            lambda: 0.5,
            top_k: 5,
            max_candidates: 50,
        };
        let result = mmr_rerank(candidates, &config);
        assert_eq!(result.len(), 1);
    }

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
