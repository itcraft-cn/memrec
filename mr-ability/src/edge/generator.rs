//! # 边生成器
//!
//! [`EdgeGenerator`] trait 定义边自动生成的接口，
//! [`MockEdgeGenerator`] 提供基于规则的测试实现。
//!
//! ## 边生成策略
//!
//! - **SameProject**: 基于元数据自动生成，无需 LLM
//! - **Similar/CausedBy/Contradicts**: 语义关系，需要 LLM 判断
//! - **Evolves/References**: 演进/引用关系，需要 LLM 判断

use crate::storage::{EdgeStorage, MemoryStorage};
use anyhow::Result;
use async_trait::async_trait;
use mr_common::{EdgeType, Memory, MemoryEdge};
use uuid::Uuid;

/// 边生成器 trait。
///
/// 根据新记忆和候选记忆列表，自动生成边关系。
#[async_trait]
pub trait EdgeGenerator: Send + Sync {
    /// 为新记忆生成边关系。
    ///
    /// # 参数
    ///
    /// - `new_memory`: 新添加的记忆
    /// - `candidate_memories`: 候选关联记忆（通常通过向量检索获得）
    /// - `edge_store`: 边存储
    ///
    /// # 返回
    ///
    /// 生成的边列表。
    async fn generate(
        &self,
        new_memory: &Memory,
        candidate_memories: &[Memory],
        edge_store: &dyn EdgeStorage,
    ) -> Result<Vec<MemoryEdge>>;
}

/// Mock 边生成器。
///
/// 基于关键词匹配规则生成边，用于测试。
pub struct MockEdgeGenerator {
    min_similarity: f32,
    max_edges_per_memory: usize,
}

impl MockEdgeGenerator {
    pub fn new(min_similarity: f32, max_edges_per_memory: usize) -> Self {
        Self {
            min_similarity: min_similarity.clamp(0.0, 1.0),
            max_edges_per_memory,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(0.5, 10)
    }

    fn calculate_similarity(content_a: &str, content_b: &str) -> f32 {
        let a_lower = content_a.to_lowercase();
        let b_lower = content_b.to_lowercase();

        let words_a: std::collections::HashSet<&str> = a_lower.split_whitespace().collect();
        let words_b: std::collections::HashSet<&str> = b_lower.split_whitespace().collect();

        if words_a.is_empty() || words_b.is_empty() {
            return 0.0;
        }

        let intersection = words_a.intersection(&words_b).count();
        let union = words_a.union(&words_b).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    fn infer_edge_type(&self, _content_a: &str, _content_b: &str) -> EdgeType {
        EdgeType::Similar
    }

    fn generate_evidence(edge_type: EdgeType, similarity: f32) -> String {
        format!(
            "Auto-generated {} edge (similarity: {:.2})",
            edge_type, similarity
        )
    }
}

#[async_trait]
impl EdgeGenerator for MockEdgeGenerator {
    async fn generate(
        &self,
        new_memory: &Memory,
        candidate_memories: &[Memory],
        edge_store: &dyn EdgeStorage,
    ) -> Result<Vec<MemoryEdge>> {
        let existing_edges = edge_store.neighbors(&new_memory.id).await?;
        let existing_targets: std::collections::HashSet<Uuid> = existing_edges
            .iter()
            .map(|e| {
                if e.source_id == new_memory.id {
                    e.target_id
                } else {
                    e.source_id
                }
            })
            .collect();

        let mut edges = Vec::new();
        let mut edge_count = existing_edges.len();

        for candidate in candidate_memories {
            if edge_count >= self.max_edges_per_memory {
                break;
            }

            if existing_targets.contains(&candidate.id) {
                continue;
            }

            if new_memory.id == candidate.id {
                continue;
            }

            let similarity = Self::calculate_similarity(&new_memory.content, &candidate.content);

            if similarity >= self.min_similarity {
                let edge_type = self.infer_edge_type(&new_memory.content, &candidate.content);
                let evidence = Self::generate_evidence(edge_type, similarity);

                let edge =
                    MemoryEdge::new(new_memory.id, candidate.id, edge_type, similarity, evidence)
                        .auto_generated();

                edges.push(edge);
                edge_count += 1;
            }
        }

        Ok(edges)
    }
}

/// 项目边生成器。
///
/// 基于 `project_id` 元数据自动生成 SameProject 边。
pub struct ProjectEdgeGenerator {
    max_edges_per_project: usize,
}

impl ProjectEdgeGenerator {
    pub fn new(max_edges_per_project: usize) -> Self {
        Self {
            max_edges_per_project,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(5)
    }

    pub async fn generate_project_edges(
        &self,
        memory: &Memory,
        memory_store: &dyn MemoryStorage,
        edge_store: &dyn EdgeStorage,
    ) -> Result<Vec<MemoryEdge>> {
        let project_id = match memory.project_id {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };

        let project_memories = memory_store.list_by_project(&project_id).await?;

        let existing = edge_store.neighbors(&memory.id).await?;
        let existing_ids: std::collections::HashSet<Uuid> = existing
            .iter()
            .filter(|e| e.edge_type == EdgeType::SameProject)
            .map(|e| {
                if e.source_id == memory.id {
                    e.target_id
                } else {
                    e.source_id
                }
            })
            .collect();

        let mut edges = Vec::new();
        for other in project_memories {
            if other.id == memory.id {
                continue;
            }
            if existing_ids.contains(&other.id) {
                continue;
            }
            if edges.len() >= self.max_edges_per_project {
                break;
            }
            edges.push(
                MemoryEdge::new(
                    memory.id,
                    other.id,
                    EdgeType::SameProject,
                    1.0,
                    format!("Same project: {:?}", memory.project_id),
                )
                .auto_generated(),
            );
        }

        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{EdgeStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    async fn create_test_edge_store() -> EdgeStore {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        EdgeStore::new(rocksdb)
    }

    fn create_test_memory(content: &str) -> Memory {
        Memory::new(content.to_string(), MemoryType::Knowledge)
    }

    #[tokio::test]
    async fn test_mock_generator_similar_edges() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::with_defaults();

        let memory_a = create_test_memory("Tokio is a great async runtime for Rust");
        let memory_b = create_test_memory("async-std is another async runtime for Rust");
        let memory_c = create_test_memory("Database optimization techniques");

        let edges = generator
            .generate(&memory_a, &[memory_b.clone(), memory_c], &edge_store)
            .await
            .unwrap();

        assert!(!edges.is_empty());
        assert!(edges[0].edge_type == EdgeType::Similar);
        assert!(edges[0].auto_generated);
    }

    #[tokio::test]
    async fn test_mock_generator_respects_max_edges() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::new(0.3, 2);

        let memory = create_test_memory("Test memory with keywords");
        let candidates: Vec<Memory> = (0..10)
            .map(|i| create_test_memory(&format!("Test memory with keywords {}", i)))
            .collect();

        let edges = generator
            .generate(&memory, &candidates, &edge_store)
            .await
            .unwrap();
        assert!(edges.len() <= 2);
    }

    #[tokio::test]
    async fn test_mock_generator_avoids_self_edge() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::with_defaults();

        let memory = create_test_memory("Test content");
        let edges = generator
            .generate(&memory, std::slice::from_ref(&memory), &edge_store)
            .await
            .unwrap();

        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn test_mock_generator_low_similarity_filtered() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::new(0.8, 10);

        let memory_a = create_test_memory("Rust programming language");
        let memory_b = create_test_memory("Cooking recipes for dinner");

        let edges = generator
            .generate(&memory_a, &[memory_b], &edge_store)
            .await
            .unwrap();

        assert!(edges.is_empty());
    }
}
