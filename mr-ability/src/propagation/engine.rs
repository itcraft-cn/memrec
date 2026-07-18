//! # 传播引擎
//!
//! 实现 Dream 整合后的增量传播流程。

use anyhow::Result;
use mr_common::{Memory, PropagationConfig, PropagationDelta};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use super::generator::DeltaGenerator;
use crate::storage::{EdgeStorage, MemoryStorage, PropagationStorage};

/// 传播结果。
#[derive(Debug, Clone)]
pub struct PropagationResult {
    pub source_id: Uuid,
    pub neighbor_count: usize,
    pub delta_count: usize,
    pub delta_ids: Vec<Uuid>,
}

/// 传播引擎。
pub struct PropagationEngine {
    config: PropagationConfig,
    memory_storage: Arc<dyn MemoryStorage>,
    edge_storage: Arc<dyn EdgeStorage>,
    propagation_storage: Arc<dyn PropagationStorage>,
    generator: Arc<dyn DeltaGenerator>,
}

impl PropagationEngine {
    pub fn new(
        config: PropagationConfig,
        memory_storage: Arc<dyn MemoryStorage>,
        edge_storage: Arc<dyn EdgeStorage>,
        propagation_storage: Arc<dyn PropagationStorage>,
        generator: Arc<dyn DeltaGenerator>,
    ) -> Self {
        Self {
            config,
            memory_storage,
            edge_storage,
            propagation_storage,
            generator,
        }
    }

    /// 从整合后的记忆触发传播。
    pub async fn propagate(&self, consolidated: &Memory) -> Result<PropagationResult> {
        let neighbors = self.get_neighbors(consolidated.id).await?;
        let neighbor_count = neighbors.len();

        info!(
            "Propagation: found {} neighbors for memory {}",
            neighbor_count, consolidated.id
        );

        if neighbors.is_empty() {
            return Ok(PropagationResult {
                source_id: consolidated.id,
                neighbor_count: 0,
                delta_count: 0,
                delta_ids: vec![],
            });
        }

        let neighbor_memories = self.load_neighbor_memories(&neighbors).await?;

        let neighbor_data: Vec<(Memory, f32)> = neighbors
            .iter()
            .filter_map(|edge| {
                neighbor_memories.iter().find_map(|m| {
                    if m.id == edge.target_id {
                        Some((m.clone(), edge.weight))
                    } else {
                        None
                    }
                })
            })
            .take(self.config.max_neighbors)
            .collect();

        let deltas = self
            .generator
            .generate_batch(consolidated, &neighbor_data)
            .await?;

        let delta_count = deltas.len();
        let delta_ids: Vec<Uuid> = deltas.iter().map(|d| d.id).collect();

        for delta in &deltas {
            self.propagation_storage.save(delta).await?;
        }

        info!(
            "Propagation: generated {} deltas for memory {}",
            delta_count, consolidated.id
        );

        Ok(PropagationResult {
            source_id: consolidated.id,
            neighbor_count,
            delta_count,
            delta_ids,
        })
    }

    /// 应用待处理的增量。
    pub async fn apply_pending(&self, batch_size: usize) -> Result<usize> {
        let mut applied = 0;

        for _ in 0..batch_size {
            let pending = self.get_next_pending().await?;
            if let Some(delta) = pending {
                if self.propagation_storage.mark_applied(&delta.id).await? {
                    applied += 1;
                }
            } else {
                break;
            }
        }

        Ok(applied)
    }

    async fn get_neighbors(&self, memory_id: Uuid) -> Result<Vec<mr_common::MemoryEdge>> {
        let edges = self.edge_storage.neighbors(&memory_id).await?;

        let mut edges: Vec<_> = edges.into_iter().collect();
        edges.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
        edges.truncate(self.config.max_neighbors);

        Ok(edges)
    }

    async fn load_neighbor_memories(
        &self,
        edges: &[mr_common::MemoryEdge],
    ) -> Result<Vec<Memory>> {
        let mut memories = Vec::new();
        for edge in edges {
            if let Some(m) = self.memory_storage.get(&edge.target_id).await? {
                memories.push(m);
            }
        }
        Ok(memories)
    }

    async fn get_next_pending(&self) -> Result<Option<PropagationDelta>> {
        let all_memories = self.memory_storage.list(1000).await?;

        for memory in all_memories {
            let pending = self.propagation_storage.list_pending(&memory.id).await?;
            if let Some(delta) = pending.into_iter().next() {
                return Ok(Some(delta));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_propagation_result_debug() {
        let result = PropagationResult {
            source_id: Uuid::nil(),
            neighbor_count: 5,
            delta_count: 3,
            delta_ids: vec![Uuid::nil()],
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("neighbor_count"));
        assert!(debug_str.contains("5"));
    }
}
