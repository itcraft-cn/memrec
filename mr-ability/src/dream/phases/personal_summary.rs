//! # Phase 2: 个人总体记忆提炼
//!
//! 聚合全局记忆，生成个人偏好和知识总结。

use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;
use uuid::Uuid;

use mr_common::types::{Memory, MemoryScope, MemoryType};
use crate::storage::MemoryStorage;

use super::PhaseResult;

#[allow(dead_code)]
pub struct PersonalSummarizer {
    storage: Arc<dyn MemoryStorage>,
    batch_size: usize,
    batch_interval_ms: u64,
}

impl PersonalSummarizer {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        batch_size: usize,
        batch_interval_ms: u64,
    ) -> Self {
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

        let summary_content = format!(
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

        let summary_memory = Memory::new(summary_content, MemoryType::Knowledge)
            .with_tags(vec!["personal-summary".to_string()])
            .with_scope(MemoryScope::Global);

        let created = match self.storage.add(&summary_memory).await {
            Ok(_) => {
                info!(target: "dream", "Dream Phase 2: created personal summary memory");
                1
            }
            Err(e) => {
                tracing::warn!("Failed to create personal summary: {}", e);
                0
            }
        };

        info!(
            target: "dream",
            "Dream Phase 2 completed: processed {} memories, created {}",
            processed, created
        );

        PhaseResult::ok("PersonalSummary", processed, created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_personal_summarizer_empty() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let summarizer = PersonalSummarizer::new(storage, 100, 10);
        let result = summarizer.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 0);
    }

    #[tokio::test]
    async fn test_personal_summarizer_with_memories() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

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
