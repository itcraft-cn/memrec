//! # 增量生成器
//!
//! 定义 [`DeltaGenerator`] trait，用于生成传播增量。
//! 提供 [`MockDeltaGenerator`] 用于测试。

use anyhow::Result;
use async_trait::async_trait;
use mr_common::{DeltaType, Memory, PropagationDelta};

/// 增量生成器 trait。
///
/// 根据整合后的记忆和邻居记忆，生成传播增量。
#[async_trait]
pub trait DeltaGenerator: Send + Sync {
    /// 为单个邻居生成增量。
    async fn generate(
        &self,
        consolidated: &Memory,
        neighbor: &Memory,
        edge_weight: f32,
    ) -> Result<Option<PropagationDelta>>;

    /// 批量生成增量（默认实现，逐个调用）。
    async fn generate_batch(
        &self,
        consolidated: &Memory,
        neighbors: &[(Memory, f32)],
    ) -> Result<Vec<PropagationDelta>> {
        let mut deltas = Vec::new();
        for (neighbor, weight) in neighbors {
            if let Some(delta) = self.generate(consolidated, neighbor, *weight).await? {
                deltas.push(delta);
            }
        }
        Ok(deltas)
    }
}

/// Mock 增量生成器，用于测试。
///
/// 根据简单规则生成增量，不调用 LLM。
pub struct MockDeltaGenerator {
    min_confidence: f32,
}

impl MockDeltaGenerator {
    pub fn new(min_confidence: f32) -> Self {
        Self { min_confidence }
    }
}

impl Default for MockDeltaGenerator {
    fn default() -> Self {
        Self::new(0.5)
    }
}

#[async_trait]
impl DeltaGenerator for MockDeltaGenerator {
    async fn generate(
        &self,
        consolidated: &Memory,
        neighbor: &Memory,
        edge_weight: f32,
    ) -> Result<Option<PropagationDelta>> {
        if edge_weight < 0.1 {
            return Ok(None);
        }

        let delta_type = classify_delta(consolidated, neighbor);
        let confidence = (edge_weight * 0.9).min(1.0).max(self.min_confidence);

        if confidence < self.min_confidence {
            return Ok(None);
        }

        let content = format!(
            "Mock propagation from '{}' to '{}': type={}",
            consolidated.content.chars().take(30).collect::<String>(),
            neighbor.content.chars().take(30).collect::<String>(),
            delta_type
        );

        Ok(Some(PropagationDelta::new(
            consolidated.id,
            neighbor.id,
            delta_type,
            content,
            confidence,
        )))
    }
}

fn classify_delta(consolidated: &Memory, neighbor: &Memory) -> DeltaType {
    if consolidated.memory_type != neighbor.memory_type {
        DeltaType::KnowledgeUpdate
    } else if consolidated.importance > neighbor.importance + 0.3 {
        DeltaType::Refinement
    } else {
        DeltaType::KnowledgeUpdate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_common::MemoryType;

    fn make_memory(content: &str, mtype: MemoryType, importance: f32) -> Memory {
        let mut m = Memory::new(content.to_string(), mtype);
        m.importance = importance;
        m
    }

    #[tokio::test]
    async fn test_generate_low_weight() {
        let gen = MockDeltaGenerator::default();
        let consolidated = make_memory("consolidated", MemoryType::Knowledge, 0.8);
        let neighbor = make_memory("neighbor", MemoryType::Knowledge, 0.7);

        let result = gen.generate(&consolidated, &neighbor, 0.05).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_generate_success() {
        let gen = MockDeltaGenerator::default();
        let consolidated = make_memory("consolidated", MemoryType::Knowledge, 0.8);
        let neighbor = make_memory("neighbor", MemoryType::Decision, 0.7);

        let result = gen.generate(&consolidated, &neighbor, 0.8).await.unwrap();
        assert!(result.is_some());

        let delta = result.unwrap();
        assert_eq!(delta.source_id, consolidated.id);
        assert_eq!(delta.target_id, neighbor.id);
        assert_eq!(delta.delta_type, DeltaType::KnowledgeUpdate);
        assert!(delta.confidence > 0.0);
    }

    #[tokio::test]
    async fn test_generate_batch() {
        let gen = MockDeltaGenerator::default();
        let consolidated = make_memory("consolidated", MemoryType::Knowledge, 0.8);

        let neighbors = vec![
            (make_memory("n1", MemoryType::Decision, 0.7), 0.8),
            (make_memory("n2", MemoryType::Knowledge, 0.6), 0.6),
            (make_memory("n3", MemoryType::Knowledge, 0.5), 0.05),
        ];

        let deltas = gen.generate_batch(&consolidated, &neighbors).await.unwrap();
        assert_eq!(deltas.len(), 2);
    }

    #[test]
    fn test_classify_delta() {
        let m1 = make_memory("a", MemoryType::Knowledge, 0.5);
        let m2 = make_memory("b", MemoryType::Decision, 0.5);
        assert_eq!(classify_delta(&m1, &m2), DeltaType::KnowledgeUpdate);

        let m3 = make_memory("c", MemoryType::Knowledge, 0.9);
        let m4 = make_memory("d", MemoryType::Knowledge, 0.5);
        assert_eq!(classify_delta(&m3, &m4), DeltaType::Refinement);
    }
}
