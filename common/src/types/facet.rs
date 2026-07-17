//! # Facet 结构化记忆
//!
//! 定义 [`MemoryFacet`] 结构，将扁平记忆提炼为结构化主题。
//!
//! ## Facet 结构
//!
//! 每个 Facet 包含：
//! - `theme`: 主题描述（如 "偏好 Rust 异步编程"）
//! - `confidence`: 置信度（0-1）
//! - `keywords`: 关键词标签
//! - `evidence`: 来源记忆 ID 列表
//!
//! ## 设计参考
//!
//! 基于 MemRec 论文 (arXiv:2601.08816) Section 2.1.1:
//! - Facet 结构: `{theme, confidence, supporting_neighbors}`
//! - N_f=7 facets per memory, k=16 neighbors

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 记忆的主题维度，描述记忆的一个方面。
///
/// 一条记忆可包含多个 Facet，每个 Facet 描述一个主题。
/// Facet 用于提升检索精度，支持主题级别的匹配。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFacet {
    pub id: Uuid,
    pub memory_id: Uuid,
    pub theme: String,
    pub confidence: f32,
    pub keywords: Vec<String>,
    pub evidence: Vec<Uuid>,
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
}

impl MemoryFacet {
    pub fn new(memory_id: Uuid, theme: String, confidence: f32, keywords: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            memory_id,
            theme,
            confidence: confidence.clamp(0.0, 1.0),
            keywords,
            evidence: Vec::new(),
            embedding: None,
            created_at: now,
            last_accessed: now,
        }
    }

    pub fn new_with_evidence(
        memory_id: Uuid,
        theme: String,
        confidence: f32,
        keywords: Vec<String>,
        evidence: Vec<Uuid>,
    ) -> Self {
        let mut facet = Self::new(memory_id, theme, confidence, keywords);
        facet.evidence = evidence;
        facet
    }

    pub fn with_evidence(mut self, evidence: Vec<Uuid>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    pub fn access(&mut self) {
        self.last_accessed = Utc::now();
    }

    pub fn is_valid(&self) -> bool {
        !self.theme.is_empty() && self.confidence > 0.0 && self.confidence <= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_facet_creation() {
        let memory_id = Uuid::new_v4();
        let facet = MemoryFacet::new(
            memory_id,
            "偏好 Rust 异步编程".to_string(),
            0.85,
            vec!["tokio".to_string(), "async".to_string()],
        );

        assert!(!facet.id.to_string().is_empty());
        assert_eq!(facet.memory_id, memory_id);
        assert_eq!(facet.theme, "偏好 Rust 异步编程");
        assert_eq!(facet.confidence, 0.85);
        assert_eq!(facet.keywords.len(), 2);
        assert!(facet.evidence.is_empty());
        assert!(facet.embedding.is_none());
    }

    #[test]
    fn test_facet_with_evidence() {
        let memory_id = Uuid::new_v4();
        let evidence = vec![Uuid::new_v4(), Uuid::new_v4()];

        let facet = MemoryFacet::new(memory_id, "test".to_string(), 0.8, vec![])
            .with_evidence(evidence.clone());

        assert_eq!(facet.evidence.len(), 2);
    }

    #[test]
    fn test_facet_confidence_clamp() {
        let facet1 = MemoryFacet::new(Uuid::new_v4(), "test".to_string(), 1.5, vec![]);
        assert_eq!(facet1.confidence, 1.0);

        let facet2 = MemoryFacet::new(Uuid::new_v4(), "test".to_string(), -0.1, vec![]);
        assert_eq!(facet2.confidence, 0.0);
    }

    #[test]
    fn test_facet_serde() {
        let memory_id = Uuid::new_v4();
        let facet = MemoryFacet::new(
            memory_id,
            "test theme".to_string(),
            0.9,
            vec!["kw1".to_string()],
        );

        let json = serde_json::to_string(&facet).unwrap();
        let parsed: MemoryFacet = serde_json::from_str(&json).unwrap();

        assert_eq!(facet.id, parsed.id);
        assert_eq!(facet.theme, parsed.theme);
        assert_eq!(facet.confidence, parsed.confidence);
    }

    #[test]
    fn test_facet_is_valid() {
        let valid = MemoryFacet::new(Uuid::new_v4(), "test".to_string(), 0.8, vec![]);
        assert!(valid.is_valid());

        let invalid_theme = MemoryFacet::new(Uuid::new_v4(), "".to_string(), 0.8, vec![]);
        assert!(!invalid_theme.is_valid());

        let invalid_confidence = MemoryFacet::new(Uuid::new_v4(), "test".to_string(), 0.0, vec![]);
        assert!(!invalid_confidence.is_valid());
    }
}
