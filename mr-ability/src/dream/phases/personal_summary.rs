//! # Phase 2: 个人总体记忆提炼
//!
//! 聚合全局记忆，生成个人偏好和知识总结。
//!
//! 注意：本阶段只维护一条 `personal-summary` 摘要记忆（upsert），
//! 内容无变化时不写入，避免重复垃圾记忆污染记忆库。

use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;
use uuid::Uuid;

use crate::storage::MemoryStorage;

use super::{upsert_summary, PhaseResult};

#[allow(dead_code)]
pub struct PersonalSummarizer {
    storage: Arc<dyn MemoryStorage>,
    batch_size: usize,
    batch_interval_ms: u64,
}

impl PersonalSummarizer {
    pub fn new(storage: Arc<dyn MemoryStorage>, batch_size: usize, batch_interval_ms: u64) -> Self {
        Self {
            storage,
            batch_size,
            batch_interval_ms,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 2 started: PersonalSummary");

        let global_memories = match self.storage.list_by_project(&Uuid::nil()).await {
            Ok(memories) => memories,
            Err(e) => return PhaseResult::err("PersonalSummary", e.to_string()),
        };

        if global_memories.is_empty() {
            info!(target: "dream", "Dream Phase 2 skipped: no global memories");
            return PhaseResult::ok("PersonalSummary", 0, 0);
        }

        let mut type_counts: HashMap<String, usize> = HashMap::new();
        let mut tag_counts: HashMap<String, usize> = HashMap::new();
        let mut processed = 0;

        for memory in &global_memories {
            *type_counts
                .entry(memory.memory_type.to_string())
                .or_insert(0) += 1;

            for tag in &memory.tags {
                *tag_counts.entry(tag.clone()).or_insert(0) += 1;
            }
            processed += 1;
        }

        let top_tags: Vec<String> = tag_counts
            .iter()
            .filter(|(_, count)| **count >= 2)
            .map(|(tag, _)| tag.clone())
            .collect();

        let summary_memory = format!(
            "Personal knowledge profile:\n\
             - Memory types: {}\n\
             - Top interests: {}",
            type_counts
                .iter()
                .map(|(t, c)| format!("{}({})", t, c))
                .collect::<Vec<_>>()
                .join(", "),
            top_tags.join(", ")
        );

        // 系统摘要记忆：importance 调低（0.3）避免挤占真实记忆检索权重，source=System
        let (created, deleted) =
            match upsert_summary(&self.storage, "personal-summary", summary_memory, 0.3).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!("Failed to upsert personal summary: {}", e);
                    (0, 0)
                }
            };

        info!(
            target: "dream",
            "Dream Phase 2 completed: processed {} memories, created {}, removed {}",
            processed, created, deleted
        );

        PhaseResult::ok("PersonalSummary", processed, created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::types::{Memory, MemoryScope, MemoryType};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_personal_summarizer_empty() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let summarizer = PersonalSummarizer::new(storage, 100, 10);
        let result = summarizer.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 0);
    }

    #[tokio::test]
    async fn test_personal_summarizer_with_memories() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let mut m1 = Memory::new("global memory 1".to_string(), MemoryType::Knowledge)
            .with_project(Uuid::nil())
            .with_scope(MemoryScope::Global);
        m1.tags = vec!["rust".to_string()];
        storage.save(&m1).await.unwrap();

        let mut m2 = Memory::new("global memory 2".to_string(), MemoryType::Decision)
            .with_project(Uuid::nil())
            .with_scope(MemoryScope::Global);
        m2.tags = vec!["rust".to_string()];
        storage.save(&m2).await.unwrap();

        let summarizer = PersonalSummarizer::new(storage, 100, 10);
        let result = summarizer.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 2);
    }
}
