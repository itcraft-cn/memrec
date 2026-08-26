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

/// Dream 统计摘要标签：每标签仅保留最新一条，其余滚动删除。
///
/// 兜底保护：即使某次 Phase 1/2 执行失败或历史遗留产生多条摘要，
/// 也会在清理阶段收敛到单条，防止重复摘要污染记忆库。
const SUMMARY_TAGS: &[&str] = &["personal-summary", "cross-project"];

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

        let mut processed = 0;
        let mut cleaned = 0;

        if old_memories.is_empty() {
            info!(target: "dream", "Dream Phase 3: no old memories, skipping regular cleanup");
        } else {
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
        }

        info!(
            target: "dream",
            "Dream Phase 3 completed: processed {} memories, cleaned {}",
            processed, cleaned
        );

        // 滚动清理 Dream 统计摘要：每标签仅保留最新一条
        for tag in SUMMARY_TAGS {
            let mut list = match self.storage.list_by_tag(tag, 1000).await {
                Ok(list) => list,
                Err(e) => {
                    tracing::warn!("Failed to list summary tag {}: {}", tag, e);
                    continue;
                }
            };

            list.sort_by_key(|b| std::cmp::Reverse(b.created_at));

            if list.len() > 1 {
                let removed = list.len() - 1;
                for old in list.drain(1..) {
                    if let Err(e) = self.storage.delete(&old.id).await {
                        tracing::warn!("Failed to delete stale summary {}: {}", old.id, e);
                    } else {
                        cleaned += 1;
                    }
                }
                info!(
                    target: "dream",
                    "Dream Phase 3: rolled back {} stale '{}' summaries, keeping the latest",
                    removed,
                    tag
                );
            }
        }

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

    #[tokio::test]
    async fn test_memory_cleaner_rolls_back_summaries() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        // 预置 3 条 personal-summary 摘要（不同时间），模拟重复垃圾记忆
        for i in 0..3 {
            let mut m = Memory::new(format!("summary {}", i), MemoryType::Knowledge);
            m.tags = vec!["personal-summary".to_string()];
            m.importance = 0.3;
            m.created_at = Utc::now() - chrono::Duration::hours(i as i64 + 1);
            storage.save(&m).await.unwrap();
        }

        // 预置 3 条 cross-project 摘要
        for i in 0..3 {
            let mut m = Memory::new(format!("cross {}", i), MemoryType::Knowledge);
            m.tags = vec!["cross-project".to_string()];
            m.importance = 0.3;
            m.created_at = Utc::now() - chrono::Duration::hours(i as i64 + 5);
            storage.save(&m).await.unwrap();
        }

        let cleaner = MemoryCleaner::new(storage.clone(), 100, 10, 90, 0.1);
        let result = cleaner.execute().await;

        assert!(result.success);
        // 每标签收敛到最新 1 条
        let personal = storage.list_by_tag("personal-summary", 100).await.unwrap();
        assert_eq!(personal.len(), 1);
        assert_eq!(personal[0].content, "summary 0");

        let cross = storage.list_by_tag("cross-project", 100).await.unwrap();
        assert_eq!(cross.len(), 1);
        assert_eq!(cross[0].content, "cross 0");
    }
}
