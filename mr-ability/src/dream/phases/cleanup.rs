//! # Phase 3: 记忆清理
//!
//! 清理过期和低重要性记忆。

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::time::sleep;
use tracing::info;

use crate::storage::MemoryStorage;

use super::PhaseResult;

pub struct MemoryCleaner {
    storage: Arc<dyn MemoryStorage>,
    batch_size: usize,
    batch_interval_ms: u64,
    inactive_days: u64,
    importance_threshold: f32,
}

impl MemoryCleaner {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        batch_size: usize,
        batch_interval_ms: u64,
        inactive_days: u64,
        importance_threshold: f32,
    ) -> Self {
        Self {
            storage,
            batch_size,
            batch_interval_ms,
            inactive_days,
            importance_threshold,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 3 started: Cleanup");

        let cutoff = Utc::now() - chrono::Duration::days(self.inactive_days as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let old_memories = match self.storage.list_older_than(&cutoff_str, 10000).await {
            Ok(memories) => memories,
            Err(e) => return PhaseResult::err("Cleanup", e.to_string()),
        };

        if old_memories.is_empty() {
            info!(target: "dream", "Dream Phase 3 skipped: no old memories");
            return PhaseResult::ok("Cleanup", 0, 0);
        }

        let mut processed = 0;
        let mut cleaned = 0;

        for batch in old_memories.chunks(self.batch_size) {
            for memory in batch {
                if memory.importance < self.importance_threshold {
                    if let Err(e) = self.storage.delete(&memory.id).await {
                        tracing::warn!("Failed to delete memory {}: {}", memory.id, e);
                    } else {
                        cleaned += 1;
                    }
                }
                processed += 1;
            }

            sleep(Duration::from_millis(self.batch_interval_ms)).await;
        }

        info!(
            target: "dream",
            "Dream Phase 3 completed: processed {} memories, cleaned {}",
            processed, cleaned
        );

        PhaseResult::ok("Cleanup", processed, cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::types::{Memory, MemoryType};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_memory_cleaner_empty() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let cleaner = MemoryCleaner::new(storage, 100, 10, 90, 0.1);
        let result = cleaner.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 0);
    }

    #[tokio::test]
    async fn test_memory_cleaner_with_memories() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let mut m1 = Memory::new("old low importance".to_string(), MemoryType::Knowledge);
        m1.created_at = Utc::now() - chrono::Duration::days(100);
        m1.importance = 0.05;
        storage.save(&m1).await.unwrap();

        let mut m2 = Memory::new("old high importance".to_string(), MemoryType::Knowledge);
        m2.created_at = Utc::now() - chrono::Duration::days(100);
        m2.importance = 0.8;
        storage.save(&m2).await.unwrap();

        let cleaner = MemoryCleaner::new(storage.clone(), 100, 10, 90, 0.1);
        let result = cleaner.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 2);
    }
}
