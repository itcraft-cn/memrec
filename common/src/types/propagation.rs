//! # Dream 传播增量
//!
//! 定义 [`PropagationDelta`] 和 [`DeltaType`]，用于 Dream 整合后的增量传播。
//!
//! ## 设计参考
//!
//! 基于 MemRec 论文 (arXiv:2601.08816) Section 3.2:
//! - 异步批量传播（Asynchronous Batch Propagation）
//! - Information Bottleneck 控制传播信息量

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 增量类型枚举，描述传播内容的语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaType {
    /// 知识更新
    KnowledgeUpdate,
    /// 矛盾发现
    Contradiction,
    /// 精炼补充
    Refinement,
    /// 过时标记
    Obsolescence,
}

impl DeltaType {
    /// 优先级（数值越大优先级越高）。
    pub fn priority(&self) -> u8 {
        match self {
            DeltaType::Contradiction => 4,
            DeltaType::Obsolescence => 3,
            DeltaType::KnowledgeUpdate => 2,
            DeltaType::Refinement => 1,
        }
    }
}

impl std::fmt::Display for DeltaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeltaType::KnowledgeUpdate => write!(f, "knowledge_update"),
            DeltaType::Contradiction => write!(f, "contradiction"),
            DeltaType::Refinement => write!(f, "refinement"),
            DeltaType::Obsolescence => write!(f, "obsolescence"),
        }
    }
}

impl std::str::FromStr for DeltaType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "knowledge_update" => Ok(DeltaType::KnowledgeUpdate),
            "contradiction" => Ok(DeltaType::Contradiction),
            "refinement" => Ok(DeltaType::Refinement),
            "obsolescence" => Ok(DeltaType::Obsolescence),
            _ => Err(format!("Unknown delta type: {}", s)),
        }
    }
}

/// 传播增量，描述一次传播的内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationDelta {
    pub id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub delta_type: DeltaType,
    pub content: String,
    pub token_count: u32,
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
    pub applied: bool,
    pub applied_at: Option<DateTime<Utc>>,
}

impl PropagationDelta {
    pub fn new(
        source_id: Uuid,
        target_id: Uuid,
        delta_type: DeltaType,
        content: String,
        confidence: f32,
    ) -> Self {
        let token_count = Self::estimate_token_count(&content);
        Self {
            id: Uuid::new_v4(),
            source_id,
            target_id,
            delta_type,
            content,
            token_count,
            confidence: confidence.clamp(0.0, 1.0),
            created_at: Utc::now(),
            applied: false,
            applied_at: None,
        }
    }

    pub fn mark_applied(mut self) -> Self {
        self.applied = true;
        self.applied_at = Some(Utc::now());
        self
    }

    fn estimate_token_count(content: &str) -> u32 {
        (content.chars().count() as f32 / 2.0).ceil() as u32
    }

    pub fn is_valid(&self) -> bool {
        self.source_id != self.target_id && !self.content.is_empty() && self.confidence > 0.0
    }
}

/// 传播配置参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationConfig {
    pub max_neighbors: usize,
    pub max_delta_tokens: u32,
    pub propagation_depth: usize,
    pub batch_size: usize,
    pub retry_limit: usize,
    pub min_confidence: f32,
}

impl Default for PropagationConfig {
    fn default() -> Self {
        Self {
            max_neighbors: 8,
            max_delta_tokens: 200,
            propagation_depth: 1,
            batch_size: 10,
            retry_limit: 3,
            min_confidence: 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_delta_type_priority() {
        assert_eq!(DeltaType::Contradiction.priority(), 4);
        assert_eq!(DeltaType::Obsolescence.priority(), 3);
        assert_eq!(DeltaType::KnowledgeUpdate.priority(), 2);
        assert_eq!(DeltaType::Refinement.priority(), 1);
    }

    #[test]
    fn test_delta_type_serde() {
        let delta_type = DeltaType::KnowledgeUpdate;
        let json = serde_json::to_string(&delta_type).unwrap();
        assert_eq!(json, "\"knowledge_update\"");

        let parsed: DeltaType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, DeltaType::KnowledgeUpdate);
    }

    #[test]
    fn test_delta_type_from_str() {
        assert_eq!(
            DeltaType::from_str("contradiction").unwrap(),
            DeltaType::Contradiction
        );
        assert!(DeltaType::from_str("unknown").is_err());
    }

    #[test]
    fn test_propagation_delta_creation() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();

        let delta = PropagationDelta::new(
            source,
            target,
            DeltaType::KnowledgeUpdate,
            "Test content".to_string(),
            0.85,
        );

        assert_eq!(delta.source_id, source);
        assert_eq!(delta.target_id, target);
        assert_eq!(delta.delta_type, DeltaType::KnowledgeUpdate);
        assert_eq!(delta.confidence, 0.85);
        assert!(!delta.applied);
    }

    #[test]
    fn test_propagation_delta_mark_applied() {
        let delta = PropagationDelta::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            DeltaType::Refinement,
            "test".to_string(),
            0.9,
        );

        let applied = delta.mark_applied();
        assert!(applied.applied);
        assert!(applied.applied_at.is_some());
    }

    #[test]
    fn test_propagation_delta_is_valid() {
        let id = Uuid::new_v4();
        let delta_invalid_self =
            PropagationDelta::new(id, id, DeltaType::KnowledgeUpdate, "test".to_string(), 0.8);
        assert!(!delta_invalid_self.is_valid());

        let delta_valid = PropagationDelta::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            DeltaType::KnowledgeUpdate,
            "test".to_string(),
            0.8,
        );
        assert!(delta_valid.is_valid());
    }

    #[test]
    fn test_propagation_config_default() {
        let config = PropagationConfig::default();
        assert_eq!(config.max_neighbors, 8);
        assert_eq!(config.max_delta_tokens, 200);
        assert_eq!(config.propagation_depth, 1);
    }
}
