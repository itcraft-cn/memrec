//! # Token 预算控制器
//!
//! 控制检索结果的 Token 总预算，必要时降级。

use memrec_common::{RepresentationTier, TieredMemory};

#[derive(Debug, Clone)]
pub struct BudgetController {
    max_tokens: u32,
}

impl Default for BudgetController {
    fn default() -> Self {
        BudgetController::new(4000)
    }
}

impl BudgetController {
    pub fn new(max_tokens: u32) -> Self {
        BudgetController { max_tokens }
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn estimate_token_count(text: &str) -> u32 {
        let char_count = text.chars().count();
        (char_count as f32 * 0.4) as u32
    }

    pub fn estimate_truncated_tokens(content: &str) -> u32 {
        let limit = RepresentationTier::Truncated.max_tokens().unwrap_or(200);
        let estimated = Self::estimate_token_count(content);
        estimated.min(limit)
    }

    pub fn check_and_downgrade(&self, results: &mut Vec<TieredMemory>) -> u32 {
        let total: u32 = results.iter().map(|r| r.token_count).sum();

        if total <= self.max_tokens {
            return total;
        }

        let mut current_total = total;
        let tiers = vec![
            RepresentationTier::DenseProxy,
            RepresentationTier::Summary,
            RepresentationTier::Truncated,
            RepresentationTier::Full,
        ];

        for target_tier in tiers {
            if current_total <= self.max_tokens {
                break;
            }

            for result in results.iter_mut() {
                if current_total <= self.max_tokens {
                    break;
                }

                if result.tier == target_tier {
                    if let Some(lower) = target_tier.downgrade() {
                        let old_tokens = result.token_count;
                        let new_tokens = lower.max_tokens().unwrap_or(30);
                        let diff = old_tokens.saturating_sub(new_tokens);
                        current_total = current_total.saturating_sub(diff);
                        result.tier = lower;
                        result.token_count = new_tokens;
                    }
                }
            }
        }

        results.retain(|r| {
            r.tier != RepresentationTier::DenseProxy || current_total <= self.max_tokens
        });

        results.iter().map(|r| r.token_count).sum()
    }

    pub fn remove_lowest_tier(&self, results: &mut Vec<TieredMemory>) -> u32 {
        let before: u32 = results.iter().map(|r| r.token_count).sum();
        results.retain(|r| r.tier != RepresentationTier::DenseProxy);
        let after: u32 = results.iter().map(|r| r.token_count).sum();
        before.saturating_sub(after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_tiered_memory(tier: RepresentationTier, tokens: u32) -> TieredMemory {
        TieredMemory {
            memory_id: Uuid::new_v4(),
            tier,
            content: "test".to_string(),
            score: 0.5,
            token_count: tokens,
            facet_themes: vec![],
            edge_hints: vec![],
        }
    }

    #[test]
    fn test_default_budget() {
        let controller = BudgetController::default();
        assert_eq!(controller.max_tokens(), 4000);
    }

    #[test]
    fn test_estimate_token_count() {
        let estimate = BudgetController::estimate_token_count("hello world");
        assert!(estimate > 0);
    }

    #[test]
    fn test_no_downgrade_needed() {
        let controller = BudgetController::new(1000);
        let mut results = vec![
            make_tiered_memory(RepresentationTier::Full, 300),
            make_tiered_memory(RepresentationTier::Truncated, 200),
        ];
        let total = controller.check_and_downgrade(&mut results);
        assert_eq!(total, 500);
        assert_eq!(results[0].tier, RepresentationTier::Full);
        assert_eq!(results[1].tier, RepresentationTier::Truncated);
    }

    #[test]
    fn test_downgrade_dense_proxy() {
        let controller = BudgetController::new(50);
        let mut results = vec![
            make_tiered_memory(RepresentationTier::Summary, 80),
            make_tiered_memory(RepresentationTier::DenseProxy, 30),
        ];
        let total = controller.check_and_downgrade(&mut results);
        assert!(total <= 50);
        if results.is_empty() {
            assert!(total <= 50);
        } else {
            assert!(results[0].token_count <= 50);
        }
    }

    #[test]
    fn test_remove_lowest_tier() {
        let controller = BudgetController::new(100);
        let mut results = vec![
            make_tiered_memory(RepresentationTier::Summary, 80),
            make_tiered_memory(RepresentationTier::DenseProxy, 30),
        ];
        let removed = controller.remove_lowest_tier(&mut results);
        assert_eq!(removed, 30);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tier, RepresentationTier::Summary);
    }

    #[test]
    fn test_estimated_truncated_tokens() {
        let content = "This is a long piece of text that would normally exceed the truncated limit";
        let tokens = BudgetController::estimate_truncated_tokens(content);
        assert!(tokens <= 200);
    }
}
