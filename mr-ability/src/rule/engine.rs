//! # 规则引擎
//!
//! 规则匹配与执行的入口。

use crate::rule::matcher::{CandidateInfo, RuleMatcher};
use crate::storage::{EdgeStorage, RuleStorage};
use anyhow::Result;
use mr_common::{BoostType, RetrievalRule};
use std::sync::Arc;
use uuid::Uuid;

/// 规则引擎执行结果。
#[derive(Debug, Clone)]
pub struct RuleResult {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub hit_count: usize,
    pub action: String,
}

/// 规则引擎。
pub struct RuleEngine {
    rule_storage: Arc<dyn RuleStorage>,
    edge_storage: Option<Arc<dyn EdgeStorage>>,
    matcher: RuleMatcher,
}

impl RuleEngine {
    pub fn new(rule_storage: Arc<dyn RuleStorage>) -> Self {
        Self {
            rule_storage,
            edge_storage: None,
            matcher: RuleMatcher::new(),
        }
    }

    pub fn with_edge_storage(mut self, edge_storage: Arc<dyn EdgeStorage>) -> Self {
        self.edge_storage = Some(edge_storage);
        self
    }

    /// 对候选列表应用规则。
    pub async fn apply(
        &mut self,
        query: &str,
        candidates: &mut [CandidateInfo],
    ) -> Result<Vec<RuleResult>> {
        let rules = self.rule_storage.list_by_priority().await?;

        let mut results = Vec::new();

        for rule in rules {
            if !self.matcher.matches_query(query, &rule) {
                continue;
            }

            let hit_count = self.apply_rule(&rule, candidates).await?;

            if hit_count > 0 {
                self.rule_storage.record_hit(&rule.id).await?;

                results.push(RuleResult {
                    rule_id: rule.id,
                    rule_name: rule.name.clone(),
                    hit_count,
                    action: format!("{:?}", rule.action.boost_type),
                });
            }
        }

        if !results.is_empty() {
            candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        }

        Ok(results)
    }

    async fn apply_rule(
        &self,
        rule: &RetrievalRule,
        candidates: &mut [CandidateInfo],
    ) -> Result<usize> {
        let mut hit_count = 0;

        for candidate in candidates.iter_mut() {
            if self.matcher.matches_candidate(candidate, &rule.condition) {
                hit_count += 1;
                self.apply_action(&rule.action, candidate);
            }
        }

        Ok(hit_count)
    }

    fn apply_action(&self, action: &mr_common::RuleAction, candidate: &mut CandidateInfo) {
        match action.boost_type {
            BoostType::Boost => {
                candidate.score *= action.factor;
            }
            BoostType::Suppress => {
                candidate.score *= action.factor;
            }
            BoostType::Filter => {
                candidate.score = 0.0;
            }
            BoostType::Expand => {
                // Expand action handled separately via edge_storage
            }
        }
    }

    /// 获取所有启用的规则。
    pub async fn list_rules(&self) -> Result<Vec<RetrievalRule>> {
        self.rule_storage.list(true).await
    }

    /// 统计规则命中情况。
    pub async fn stats(&self) -> Result<RuleEngineStats> {
        let rules = self.rule_storage.list(false).await?;
        let enabled = rules.iter().filter(|r| r.enabled).count();
        let total_hits: u32 = rules.iter().map(|r| r.hit_count).sum();

        Ok(RuleEngineStats {
            total_rules: rules.len(),
            enabled_rules: enabled,
            total_hits,
        })
    }
}

/// 规则引擎统计。
#[derive(Debug, Clone)]
pub struct RuleEngineStats {
    pub total_rules: usize,
    pub enabled_rules: usize,
    pub total_hits: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_result_debug() {
        let result = RuleResult {
            rule_id: Uuid::nil(),
            rule_name: "test".to_string(),
            hit_count: 5,
            action: "Boost".to_string(),
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("rule_name"));
    }

    #[test]
    fn test_rule_engine_stats() {
        let stats = RuleEngineStats {
            total_rules: 10,
            enabled_rules: 8,
            total_hits: 42,
        };

        assert_eq!(stats.total_rules, 10);
        assert_eq!(stats.enabled_rules, 8);
        assert_eq!(stats.total_hits, 42);
    }
}
