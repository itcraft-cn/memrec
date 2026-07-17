//! # 检索规则类型定义
//!
//! LLM 生成的检索规则，用于 Curate-then-Synthesize 流程。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{EdgeType, MemoryType};

/// 规则动作类型。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoostType {
    #[default]
    Boost,
    Suppress,
    Filter,
    Expand,
}

/// 时间范围条件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// 规则触发条件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleCondition {
    pub query_pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<Vec<MemoryType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<TimeRange>,
}

impl RuleCondition {
    pub fn new(query_pattern: String) -> Self {
        Self {
            query_pattern,
            ..Default::default()
        }
    }

    pub fn with_memory_types(mut self, types: Vec<MemoryType>) -> Self {
        self.memory_type = Some(types);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }
}

/// 规则执行动作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAction {
    pub boost_type: BoostType,
    pub factor: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_types: Option<Vec<EdgeType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}

impl Default for RuleAction {
    fn default() -> Self {
        Self {
            boost_type: BoostType::Boost,
            factor: 1.0,
            edge_types: None,
            max_depth: None,
        }
    }
}

impl RuleAction {
    pub fn boost(factor: f32) -> Self {
        Self {
            boost_type: BoostType::Boost,
            factor,
            ..Default::default()
        }
    }

    pub fn suppress(factor: f32) -> Self {
        Self {
            boost_type: BoostType::Suppress,
            factor,
            ..Default::default()
        }
    }

    pub fn filter() -> Self {
        Self {
            boost_type: BoostType::Filter,
            factor: 0.0,
            ..Default::default()
        }
    }

    pub fn expand(edge_types: Vec<EdgeType>, max_depth: u32) -> Self {
        Self {
            boost_type: BoostType::Expand,
            factor: 1.0,
            edge_types: Some(edge_types),
            max_depth: Some(max_depth),
        }
    }
}

/// 检索规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalRule {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub condition: RuleCondition,
    pub action: RuleAction,
    pub priority: i32,
    pub enabled: bool,
    pub hit_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub auto_generated: bool,
}

impl RetrievalRule {
    pub fn new(
        name: String,
        description: String,
        condition: RuleCondition,
        action: RuleAction,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            condition,
            action,
            priority: 50,
            enabled: true,
            hit_count: 0,
            created_at: now,
            updated_at: now,
            auto_generated: false,
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_auto_generated(mut self, auto: bool) -> Self {
        self.auto_generated = auto;
        self
    }

    pub fn increment_hit(&mut self) {
        self.hit_count += 1;
        self.updated_at = Utc::now();
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.updated_at = Utc::now();
    }
}

/// 规则统计。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleStats {
    pub rule_id: Uuid,
    pub hit_count: u32,
    pub adoption_count: u32,
    pub last_hit: Option<DateTime<Utc>>,
    pub last_adoption: Option<DateTime<Utc>>,
}

impl RuleStats {
    pub fn new(rule_id: Uuid) -> Self {
        Self {
            rule_id,
            ..Default::default()
        }
    }

    pub fn record_hit(&mut self) {
        self.hit_count += 1;
        self.last_hit = Some(Utc::now());
    }

    pub fn record_adoption(&mut self) {
        self.adoption_count += 1;
        self.last_adoption = Some(Utc::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boost_type_default() {
        assert_eq!(BoostType::default(), BoostType::Boost);
    }

    #[test]
    fn test_rule_condition_new() {
        let cond = RuleCondition::new("test".to_string());
        assert_eq!(cond.query_pattern, "test");
        assert!(cond.memory_type.is_none());
    }

    #[test]
    fn test_rule_condition_with_memory_types() {
        let cond =
            RuleCondition::new("test".to_string()).with_memory_types(vec![MemoryType::Knowledge]);
        assert!(cond.memory_type.is_some());
        assert_eq!(cond.memory_type.unwrap().len(), 1);
    }

    #[test]
    fn test_rule_action_boost() {
        let action = RuleAction::boost(1.5);
        assert_eq!(action.boost_type, BoostType::Boost);
        assert!((action.factor - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_rule_action_filter() {
        let action = RuleAction::filter();
        assert_eq!(action.boost_type, BoostType::Filter);
        assert!((action.factor).abs() < 0.001);
    }

    #[test]
    fn test_rule_action_expand() {
        let action = RuleAction::expand(vec![EdgeType::CausedBy], 2);
        assert_eq!(action.boost_type, BoostType::Expand);
        assert_eq!(action.edge_types.unwrap().len(), 1);
        assert_eq!(action.max_depth.unwrap(), 2);
    }

    #[test]
    fn test_retrieval_rule_new() {
        let rule = RetrievalRule::new(
            "test rule".to_string(),
            "description".to_string(),
            RuleCondition::new("pattern".to_string()),
            RuleAction::boost(1.5),
        );

        assert!(!rule.id.is_nil());
        assert_eq!(rule.name, "test rule");
        assert_eq!(rule.priority, 50);
        assert!(rule.enabled);
        assert_eq!(rule.hit_count, 0);
        assert!(!rule.auto_generated);
    }

    #[test]
    fn test_retrieval_rule_with_priority() {
        let rule = RetrievalRule::new(
            "test".to_string(),
            "desc".to_string(),
            RuleCondition::default(),
            RuleAction::default(),
        )
        .with_priority(100);

        assert_eq!(rule.priority, 100);
    }

    #[test]
    fn test_retrieval_rule_increment_hit() {
        let mut rule = RetrievalRule::new(
            "test".to_string(),
            "desc".to_string(),
            RuleCondition::default(),
            RuleAction::default(),
        );

        rule.increment_hit();
        assert_eq!(rule.hit_count, 1);

        rule.increment_hit();
        assert_eq!(rule.hit_count, 2);
    }

    #[test]
    fn test_rule_stats() {
        let mut stats = RuleStats::new(Uuid::new_v4());
        assert_eq!(stats.hit_count, 0);
        assert!(stats.last_hit.is_none());

        stats.record_hit();
        assert_eq!(stats.hit_count, 1);
        assert!(stats.last_hit.is_some());

        stats.record_adoption();
        assert_eq!(stats.adoption_count, 1);
        assert!(stats.last_adoption.is_some());
    }
}
