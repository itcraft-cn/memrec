//! # 分层呈现类型
//!
//! 定义 [`RepresentationTier`], [`TieredMemory`], [`EdgeHint`], [`TierThresholds`] 等类型，
//! 实现检索结果按相关度分层呈现。
//!
//! ## 分层策略
//!
//! | 层级 | 说明 | Token 预算 | 适用场景 |
//! |------|------|-----------|----------|
//! | Full | 完整内容 | 不限 | score ≥ 0.8 |
//! | Truncated | 截断内容 | ≤ 200 tokens | 0.5 ≤ score < 0.8 |
//! | Summary | LLM 摘要 | ≤ 80 tokens | 0.3 ≤ score < 0.5 |
//! | DenseProxy | 关键词代理 | ≤ 30 tokens | score < 0.3 |
//!
//! ## 设计参考
//!
//! 基于 MemRec 论文 (arXiv:2601.08816) Section 2.1.3:
//! - Tiered Memory Representation: Full / Truncated / Summary / DenseProxy
//! - Token Budget Allocation: 按层级分配，超预算降级

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{EdgeType, MemoryType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepresentationTier {
    Full,
    Truncated,
    Summary,
    DenseProxy,
}

impl RepresentationTier {
    pub fn max_tokens(&self) -> Option<u32> {
        match self {
            RepresentationTier::Full => None,
            RepresentationTier::Truncated => Some(200),
            RepresentationTier::Summary => Some(80),
            RepresentationTier::DenseProxy => Some(30),
        }
    }

    pub fn downgrade(&self) -> Option<RepresentationTier> {
        match self {
            RepresentationTier::Full => Some(RepresentationTier::Truncated),
            RepresentationTier::Truncated => Some(RepresentationTier::Summary),
            RepresentationTier::Summary => Some(RepresentationTier::DenseProxy),
            RepresentationTier::DenseProxy => None,
        }
    }
}

impl std::fmt::Display for RepresentationTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepresentationTier::Full => write!(f, "full"),
            RepresentationTier::Truncated => write!(f, "truncated"),
            RepresentationTier::Summary => write!(f, "summary"),
            RepresentationTier::DenseProxy => write!(f, "dense_proxy"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieredMemory {
    pub memory_id: Uuid,
    pub tier: RepresentationTier,
    pub content: String,
    pub score: f32,
    pub token_count: u32,
    /// 记忆类型。注入侧据此做预算优先级分配，调用方也用于渲染归类。
    #[serde(default)]
    pub memory_type: MemoryType,
    /// 记忆标签，供调用方展示与二次过滤。
    #[serde(default)]
    pub tags: Vec<String>,
    pub facet_themes: Vec<String>,
    pub edge_hints: Vec<EdgeHint>,
}

/// 类型预算上限规则。
///
/// 表达“某类记忆在注入预算中最多可占的比例”。未列入策略的类型不设上限，
/// 参与最后一遍剩余预算回填（低优先级类型）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeBudgetCap {
    /// 记忆类型
    pub memory_type: MemoryType,
    /// 最多占用的预算比例（0.0-1.0）
    pub ratio: f32,
}

/// 注入预算策略：按类型优先级依次占用预算，剩余额度回填低优先级类型。
///
/// 设计依据：用户偏好（Preference）跨项目复用、扇入最高，应优先获得预算；
/// 决策（Decision）次之；知识（Knowledge）再次；上下文/对话只吃剩余额度。
/// 上限是“最多占用”而非“预留”，某类候选不足时其额度自动让给后续类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeBudgetPolicy {
    /// 按优先级排列的类型上限规则
    pub caps: Vec<TypeBudgetCap>,
}

impl Default for TypeBudgetPolicy {
    fn default() -> Self {
        Self {
            caps: vec![
                TypeBudgetCap {
                    memory_type: MemoryType::Preference,
                    ratio: 0.5,
                },
                TypeBudgetCap {
                    memory_type: MemoryType::Decision,
                    ratio: 0.3,
                },
                TypeBudgetCap {
                    memory_type: MemoryType::Knowledge,
                    ratio: 0.2,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeHint {
    pub edge_type: EdgeType,
    pub target_summary: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierThresholds {
    pub full: f32,
    pub truncated: f32,
    pub summary: f32,
}

impl Default for TierThresholds {
    fn default() -> Self {
        TierThresholds {
            full: 0.8,
            truncated: 0.5,
            summary: 0.3,
        }
    }
}

impl TierThresholds {
    pub fn assign_tier(&self, score: f32) -> RepresentationTier {
        if score >= self.full {
            RepresentationTier::Full
        } else if score >= self.truncated {
            RepresentationTier::Truncated
        } else if score >= self.summary {
            RepresentationTier::Summary
        } else {
            RepresentationTier::DenseProxy
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieredSearchResult {
    pub total_results: usize,
    pub total_tokens: u32,
    pub budget: u32,
    pub results: Vec<TieredMemory>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_max_tokens() {
        assert_eq!(RepresentationTier::Full.max_tokens(), None);
        assert_eq!(RepresentationTier::Truncated.max_tokens(), Some(200));
        assert_eq!(RepresentationTier::Summary.max_tokens(), Some(80));
        assert_eq!(RepresentationTier::DenseProxy.max_tokens(), Some(30));
    }

    #[test]
    fn test_tier_downgrade() {
        assert_eq!(
            RepresentationTier::Full.downgrade(),
            Some(RepresentationTier::Truncated)
        );
        assert_eq!(
            RepresentationTier::Truncated.downgrade(),
            Some(RepresentationTier::Summary)
        );
        assert_eq!(
            RepresentationTier::Summary.downgrade(),
            Some(RepresentationTier::DenseProxy)
        );
        assert_eq!(RepresentationTier::DenseProxy.downgrade(), None);
    }

    #[test]
    fn test_tier_display() {
        assert_eq!(RepresentationTier::Full.to_string(), "full");
        assert_eq!(RepresentationTier::Truncated.to_string(), "truncated");
        assert_eq!(RepresentationTier::Summary.to_string(), "summary");
        assert_eq!(RepresentationTier::DenseProxy.to_string(), "dense_proxy");
    }

    #[test]
    fn test_thresholds_default() {
        let t = TierThresholds::default();
        assert!((t.full - 0.8).abs() < f32::EPSILON);
        assert!((t.truncated - 0.5).abs() < f32::EPSILON);
        assert!((t.summary - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_assign_tier() {
        let t = TierThresholds::default();
        assert_eq!(t.assign_tier(0.9), RepresentationTier::Full);
        assert_eq!(t.assign_tier(0.8), RepresentationTier::Full);
        assert_eq!(t.assign_tier(0.65), RepresentationTier::Truncated);
        assert_eq!(t.assign_tier(0.5), RepresentationTier::Truncated);
        assert_eq!(t.assign_tier(0.4), RepresentationTier::Summary);
        assert_eq!(t.assign_tier(0.3), RepresentationTier::Summary);
        assert_eq!(t.assign_tier(0.1), RepresentationTier::DenseProxy);
    }

    #[test]
    fn test_tiered_memory_serialization() {
        let tm = TieredMemory {
            memory_id: Uuid::new_v4(),
            tier: RepresentationTier::Summary,
            content: "test summary".to_string(),
            score: 0.42,
            token_count: 10,
            memory_type: MemoryType::Decision,
            tags: vec!["rust".to_string()],
            facet_themes: vec!["rust".to_string()],
            edge_hints: vec![],
        };
        let json = serde_json::to_string(&tm).unwrap();
        let de: TieredMemory = serde_json::from_str(&json).unwrap();
        assert_eq!(de.tier, RepresentationTier::Summary);
        assert!((de.score - 0.42).abs() < f32::EPSILON);
    }

    #[test]
    fn test_edge_hint_serialization() {
        let hint = EdgeHint {
            edge_type: EdgeType::SameProject,
            target_summary: "related memory".to_string(),
            weight: 0.7,
        };
        let json = serde_json::to_string(&hint).unwrap();
        let de: EdgeHint = serde_json::from_str(&json).unwrap();
        assert_eq!(de.edge_type, EdgeType::SameProject);
        assert!((de.weight - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_tiered_search_result_serialization() {
        let result = TieredSearchResult {
            total_results: 3,
            total_tokens: 500,
            budget: 4000,
            results: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        let de: TieredSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(de.total_results, 3);
        assert_eq!(de.budget, 4000);
    }

    #[test]
    fn test_custom_thresholds() {
        let t = TierThresholds {
            full: 0.9,
            truncated: 0.6,
            summary: 0.4,
        };
        assert_eq!(t.assign_tier(0.85), RepresentationTier::Truncated);
        assert_eq!(t.assign_tier(0.91), RepresentationTier::Full);
        assert_eq!(t.assign_tier(0.5), RepresentationTier::Summary);
        assert_eq!(t.assign_tier(0.35), RepresentationTier::DenseProxy);
    }
}
