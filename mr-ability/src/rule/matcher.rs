//! # 规则匹配器
//!
//! 判断查询和候选是否命中规则条件。

use mr_common::{MemoryType, RetrievalRule, RuleCondition};
use regex::Regex;
use std::collections::HashSet;

/// 规则匹配器。
pub struct RuleMatcher {
    compiled_patterns: std::collections::HashMap<String, Regex>,
}

impl RuleMatcher {
    pub fn new() -> Self {
        Self {
            compiled_patterns: std::collections::HashMap::new(),
        }
    }

    /// 检查规则是否匹配查询。
    pub fn matches_query(&mut self, query: &str, rule: &RetrievalRule) -> bool {
        if !rule.enabled {
            return false;
        }

        self.matches_condition(query, &rule.condition)
    }

    /// 检查条件是否匹配查询。
    pub fn matches_condition(&mut self, query: &str, condition: &RuleCondition) -> bool {
        if condition.query_pattern.is_empty() {
            return true;
        }

        let pattern = &condition.query_pattern;

        let regex = self
            .compiled_patterns
            .entry(pattern.clone())
            .or_insert_with(|| {
                let pattern_str = if pattern.contains('|') {
                    format!("(?i)({})", pattern)
                } else {
                    format!("(?i){}", regex::escape(pattern))
                };
                Regex::new(&pattern_str).unwrap_or_else(|_| Regex::new(".*").unwrap())
            });

        regex.is_match(query)
    }

    /// 检查候选记忆是否匹配规则条件。
    pub fn matches_candidate(&self, candidate: &CandidateInfo, condition: &RuleCondition) -> bool {
        if let Some(ref types) = condition.memory_type {
            if !types.contains(&candidate.memory_type) {
                return false;
            }
        }

        if let Some(ref project) = condition.project {
            if candidate.project_id.as_ref().map(|p| p.to_string()) != Some(project.clone()) {
                return false;
            }
        }

        if let Some(ref tags) = condition.tags {
            let candidate_tags: HashSet<_> = candidate.tags.iter().collect();
            let rule_tags: HashSet<_> = tags.iter().collect();
            if candidate_tags.is_disjoint(&rule_tags) {
                return false;
            }
        }

        if let Some(ref time_range) = condition.time_range {
            if candidate.created_at < time_range.start || candidate.created_at > time_range.end {
                return false;
            }
        }

        true
    }
}

impl Default for RuleMatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// 候选记忆信息（用于规则匹配）。
#[derive(Debug, Clone)]
pub struct CandidateInfo {
    pub memory_id: uuid::Uuid,
    pub memory_type: MemoryType,
    pub project_id: Option<uuid::Uuid>,
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_common::RuleAction;

    fn make_rule(pattern: &str, enabled: bool) -> RetrievalRule {
        let mut rule = RetrievalRule::new(
            "test".to_string(),
            "desc".to_string(),
            RuleCondition::new(pattern.to_string()),
            RuleAction::boost(1.0),
        );
        rule.enabled = enabled;
        rule
    }

    #[test]
    fn test_matches_query_enabled() {
        let mut matcher = RuleMatcher::new();
        let rule = make_rule("test", true);

        assert!(matcher.matches_query("this is a test query", &rule));
        assert!(!matcher.matches_query("hello world", &rule));
    }

    #[test]
    fn test_matches_query_disabled() {
        let mut matcher = RuleMatcher::new();
        let rule = make_rule("test", false);

        assert!(!matcher.matches_query("this is a test query", &rule));
    }

    #[test]
    fn test_matches_pattern_alternation() {
        let mut matcher = RuleMatcher::new();
        let condition = RuleCondition::new("性能|优化|瓶颈".to_string());

        assert!(matcher.matches_condition("性能问题", &condition));
        assert!(matcher.matches_condition("优化方案", &condition));
        assert!(matcher.matches_condition("瓶颈分析", &condition));
        assert!(!matcher.matches_condition("hello world", &condition));
    }

    #[test]
    fn test_matches_candidate_memory_type() {
        let matcher = RuleMatcher::new();
        let condition =
            RuleCondition::new("test".to_string()).with_memory_types(vec![MemoryType::Knowledge]);

        let matching = CandidateInfo {
            memory_id: uuid::Uuid::new_v4(),
            memory_type: MemoryType::Knowledge,
            project_id: None,
            tags: vec![],
            created_at: chrono::Utc::now(),
            score: 0.8,
        };

        let non_matching = CandidateInfo {
            memory_id: uuid::Uuid::new_v4(),
            memory_type: MemoryType::Decision,
            project_id: None,
            tags: vec![],
            created_at: chrono::Utc::now(),
            score: 0.8,
        };

        assert!(matcher.matches_candidate(&matching, &condition));
        assert!(!matcher.matches_candidate(&non_matching, &condition));
    }

    #[test]
    fn test_matches_candidate_tags() {
        let matcher = RuleMatcher::new();
        let condition = RuleCondition::new("test".to_string())
            .with_tags(vec!["rust".to_string(), "async".to_string()]);

        let matching = CandidateInfo {
            memory_id: uuid::Uuid::new_v4(),
            memory_type: MemoryType::Knowledge,
            project_id: None,
            tags: vec!["rust".to_string(), "tokio".to_string()],
            created_at: chrono::Utc::now(),
            score: 0.8,
        };

        let non_matching = CandidateInfo {
            memory_id: uuid::Uuid::new_v4(),
            memory_type: MemoryType::Knowledge,
            project_id: None,
            tags: vec!["java".to_string()],
            created_at: chrono::Utc::now(),
            score: 0.8,
        };

        assert!(matcher.matches_candidate(&matching, &condition));
        assert!(!matcher.matches_candidate(&non_matching, &condition));
    }
}
