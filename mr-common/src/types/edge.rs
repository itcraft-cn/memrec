//! # 记忆图边关系
//!
//! 定义 [`MemoryEdge`] 和 [`EdgeType`]，表达记忆间的语义关系。
//!
//! ## 设计参考
//!
//! 基于 MemRec 论文 (arXiv:2601.08816) Section 2.1:
//! - 协同记忆图结构
//! - 多种边类型支持语义扩散检索

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 边类型枚举，表达记忆间的语义关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// 语义相似（无向）
    Similar,
    /// 因果关系（有向）：target 导致 source
    CausedBy,
    /// 引用关系（有向）：source 引用 target
    References,
    /// 同项目（无向）
    SameProject,
    /// 同会话（无向）
    SameSession,
    /// 矛盾关系（无向）
    Contradicts,
    /// 演进关系（有向）：source 是 target 的演进
    Evolves,
}

impl EdgeType {
    /// 是否为有向边。
    pub fn is_directed(&self) -> bool {
        matches!(
            self,
            EdgeType::CausedBy | EdgeType::References | EdgeType::Evolves
        )
    }

    /// 边扩散权重（用于图遍历）。
    pub fn diffusion_weight(&self) -> f32 {
        match self {
            EdgeType::Similar => 1.0,
            EdgeType::CausedBy => 0.8,
            EdgeType::References => 0.7,
            EdgeType::SameProject => 0.5,
            EdgeType::SameSession => 0.3,
            EdgeType::Contradicts => 0.4,
            EdgeType::Evolves => 0.6,
        }
    }
}

impl std::fmt::Display for EdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeType::Similar => write!(f, "similar"),
            EdgeType::CausedBy => write!(f, "caused_by"),
            EdgeType::References => write!(f, "references"),
            EdgeType::SameProject => write!(f, "same_project"),
            EdgeType::SameSession => write!(f, "same_session"),
            EdgeType::Contradicts => write!(f, "contradicts"),
            EdgeType::Evolves => write!(f, "evolves"),
        }
    }
}

impl std::str::FromStr for EdgeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "similar" => Ok(EdgeType::Similar),
            "caused_by" => Ok(EdgeType::CausedBy),
            "references" => Ok(EdgeType::References),
            "same_project" => Ok(EdgeType::SameProject),
            "same_session" => Ok(EdgeType::SameSession),
            "contradicts" => Ok(EdgeType::Contradicts),
            "evolves" => Ok(EdgeType::Evolves),
            _ => Err(format!("Unknown edge type: {}", s)),
        }
    }
}

/// 记忆图边，表达两个记忆间的关系。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEdge {
    pub id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub edge_type: EdgeType,
    pub weight: f32,
    pub evidence: String,
    pub created_at: DateTime<Utc>,
    pub auto_generated: bool,
}

impl MemoryEdge {
    pub fn new(
        source_id: Uuid,
        target_id: Uuid,
        edge_type: EdgeType,
        weight: f32,
        evidence: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id,
            target_id,
            edge_type,
            weight: weight.clamp(0.0, 1.0),
            evidence,
            created_at: Utc::now(),
            auto_generated: false,
        }
    }

    pub fn auto_generated(mut self) -> Self {
        self.auto_generated = true;
        self
    }

    pub fn is_valid(&self) -> bool {
        self.source_id != self.target_id && self.weight > 0.0 && self.weight <= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_edge_type_is_directed() {
        assert!(!EdgeType::Similar.is_directed());
        assert!(EdgeType::CausedBy.is_directed());
        assert!(EdgeType::References.is_directed());
        assert!(!EdgeType::SameProject.is_directed());
    }

    #[test]
    fn test_edge_type_diffusion_weight() {
        assert_eq!(EdgeType::Similar.diffusion_weight(), 1.0);
        assert_eq!(EdgeType::CausedBy.diffusion_weight(), 0.8);
        assert_eq!(EdgeType::SameSession.diffusion_weight(), 0.3);
    }

    #[test]
    fn test_edge_creation() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();

        let edge = MemoryEdge::new(
            source,
            target,
            EdgeType::Similar,
            0.85,
            "test evidence".to_string(),
        );

        assert_eq!(edge.source_id, source);
        assert_eq!(edge.target_id, target);
        assert_eq!(edge.edge_type, EdgeType::Similar);
        assert_eq!(edge.weight, 0.85);
        assert!(!edge.auto_generated);
    }

    #[test]
    fn test_edge_weight_clamp() {
        let edge = MemoryEdge::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            EdgeType::Similar,
            1.5,
            "".to_string(),
        );
        assert_eq!(edge.weight, 1.0);

        let edge2 = MemoryEdge::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            EdgeType::Similar,
            -0.1,
            "".to_string(),
        );
        assert_eq!(edge2.weight, 0.0);
    }

    #[test]
    fn test_edge_auto_generated() {
        let edge = MemoryEdge::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            EdgeType::SameProject,
            1.0,
            "".to_string(),
        )
        .auto_generated();

        assert!(edge.auto_generated);
    }

    #[test]
    fn test_edge_is_valid() {
        let id = Uuid::new_v4();
        let edge_invalid_self = MemoryEdge::new(id, id, EdgeType::Similar, 0.8, "".to_string());
        assert!(!edge_invalid_self.is_valid());

        let edge_invalid_weight = MemoryEdge::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            EdgeType::Similar,
            0.0,
            "".to_string(),
        );
        assert!(!edge_invalid_weight.is_valid());

        let edge_valid = MemoryEdge::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            EdgeType::Similar,
            0.5,
            "".to_string(),
        );
        assert!(edge_valid.is_valid());
    }

    #[test]
    fn test_edge_type_serde() {
        let edge_type = EdgeType::CausedBy;
        let json = serde_json::to_string(&edge_type).unwrap();
        assert_eq!(json, "\"caused_by\"");

        let parsed: EdgeType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, EdgeType::CausedBy);
    }

    #[test]
    fn test_edge_type_from_str() {
        assert_eq!(EdgeType::from_str("similar").unwrap(), EdgeType::Similar);
        assert_eq!(
            EdgeType::from_str("SAME_PROJECT").unwrap(),
            EdgeType::SameProject
        );
        assert!(EdgeType::from_str("unknown").is_err());
    }
}
