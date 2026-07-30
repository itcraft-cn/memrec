//! # Phase 1: 交叉项目记忆提取
//!
//! 扫描所有记忆，提取跨项目共性主题。

use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;

use crate::storage::MemoryStorage;
use mr_common::types::{Memory, MemoryScope, MemoryType};

use super::PhaseResult;

#[allow(dead_code)]
pub struct CrossProjectExtractor {
    storage: Arc<dyn MemoryStorage>,
    batch_size: usize,
    batch_interval_ms: u64,
}

impl CrossProjectExtractor {
    pub fn new(storage: Arc<dyn MemoryStorage>, batch_size: usize, batch_interval_ms: u64) -> Self {
        Self {
            storage,
            batch_size,
            batch_interval_ms,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 1 started: CrossProjectExtract");

        let memories = match self.storage.list(10000).await {
            Ok(m) => m,
            Err(e) => return PhaseResult::err("CrossProjectExtract", e.to_string()),
        };

        if memories.is_empty() {
            info!(target: "dream", "Dream Phase 1 skipped: no memories found");
            return PhaseResult::ok("CrossProjectExtract", 0, 0);
        }

        let mut tag_project_count: HashMap<String, usize> = HashMap::new();

        for memory in &memories {
            let project_key = memory
                .project_id
                .map(|p| p.to_string())
                .unwrap_or_else(|| "global".to_string());

            for tag in &memory.tags {
                let key = format!("{}:{}", project_key, tag);
                *tag_project_count.entry(key).or_insert(0) += 1;
            }
        }

        let mut cross_project_tags: HashMap<String, usize> = HashMap::new();
        for key in tag_project_count.keys() {
            let parts: Vec<&str> = key.splitn(2, ':').collect();
            if parts.len() == 2 {
                let tag = parts[1];
                *cross_project_tags.entry(tag.to_string()).or_insert(0) += 1;
            }
        }

        let cross_project_tags: Vec<String> = cross_project_tags
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(tag, _)| tag.clone())
            .collect();

        let processed = memories.len();

        if cross_project_tags.is_empty() {
            info!(target: "dream", "Dream Phase 1 completed: no cross-project tags found");
            return PhaseResult::ok("CrossProjectExtract", processed, 0);
        }

        info!(
            target: "dream",
            "Dream Phase 1: found {} cross-project tags",
            cross_project_tags.len()
        );

        let summary_content = format!(
            "Cross-project common themes: {}",
            cross_project_tags.join(", ")
        );

        let global_memory = Memory::new(summary_content, MemoryType::Knowledge)
            .with_tags(vec!["cross-project".to_string()])
            .with_scope(MemoryScope::Global);

        let created = match self.storage.add(&global_memory).await {
            Ok(_) => {
                info!(target: "dream", "Dream Phase 1: created global memory for cross-project themes");
                1
            }
            Err(e) => {
                tracing::warn!("Failed to create cross-project memory: {}", e);
                0
            }
        };

        info!(
            target: "dream",
            "Dream Phase 1 completed: processed {} memories, created {}",
            processed, created
        );

        PhaseResult::ok("CrossProjectExtract", processed, created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cross_project_extractor_empty() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let extractor = CrossProjectExtractor::new(storage, 100, 10);
        let result = extractor.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 0);
    }

    #[tokio::test]
    async fn test_cross_project_extractor_with_memories() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let mut m1 = Memory::new("memory 1".to_string(), MemoryType::Knowledge);
        m1.tags = vec!["rust".to_string()];
        m1.project_id = Some(uuid::Uuid::new_v4());
        storage.save(&m1).await.unwrap();

        let mut m2 = Memory::new("memory 2".to_string(), MemoryType::Knowledge);
        m2.tags = vec!["rust".to_string()];
        m2.project_id = Some(uuid::Uuid::new_v4());
        storage.save(&m2).await.unwrap();

        let extractor = CrossProjectExtractor::new(storage, 100, 10);
        let result = extractor.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 2);
        assert_eq!(result.created_count, 1);
    }
}
