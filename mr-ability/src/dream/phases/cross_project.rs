//! # Phase 1: 交叉项目记忆提取
//!
//! 扫描所有记忆，提取跨项目共性主题。
//!
//! 注意：本阶段只维护一条 `cross-project` 摘要记忆（upsert），
//! 内容无变化时不写入，避免重复垃圾记忆污染记忆库。

use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;

use crate::storage::MemoryStorage;

use super::{upsert_summary, PhaseResult};

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

        // 系统摘要记忆：importance 调低（0.3）避免挤占真实记忆检索权重，source=System
        let (created, deleted) =
            match upsert_summary(&self.storage, "cross-project", summary_content, 0.3).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!("Failed to upsert cross-project summary: {}", e);
                    (0, 0)
                }
            };

        info!(
            target: "dream",
            "Dream Phase 1 completed: processed {} memories, created {}, removed {}",
            processed, created, deleted
        );

        PhaseResult::ok("CrossProjectExtract", processed, created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::types::{Memory, MemoryType};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cross_project_extractor_empty() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let extractor = CrossProjectExtractor::new(storage, 100, 10);
        let result = extractor.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 0);
    }

    #[tokio::test]
    async fn test_cross_project_extractor_with_memories() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

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

    #[tokio::test]
    async fn test_cross_project_extractor_upsert_idempotent() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage: Arc<dyn MemoryStorage> =
            Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let mut m1 = Memory::new("memory 1".to_string(), MemoryType::Knowledge);
        m1.tags = vec!["rust".to_string()];
        m1.project_id = Some(uuid::Uuid::new_v4());
        storage.save(&m1).await.unwrap();

        let mut m2 = Memory::new("memory 2".to_string(), MemoryType::Knowledge);
        m2.tags = vec!["rust".to_string()];
        m2.project_id = Some(uuid::Uuid::new_v4());
        storage.save(&m2).await.unwrap();

        let extractor = CrossProjectExtractor::new(storage.clone(), 100, 10);

        // 第一次执行：创建 1 条 cross-project 摘要
        let r1 = extractor.execute().await;
        assert!(r1.success);
        assert_eq!(r1.created_count, 1);
        let summary = storage.list_by_tag("cross-project", 10).await.unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].source, mr_common::types::MemorySource::System);
        assert_eq!(summary[0].importance, 0.3);

        // 第二次执行：内容相同，不再新增（upsert 幂等）
        let r2 = extractor.execute().await;
        assert!(r2.success);
        assert_eq!(r2.created_count, 0);
        let summary = storage.list_by_tag("cross-project", 10).await.unwrap();
        assert_eq!(summary.len(), 1);
    }

    #[tokio::test]
    async fn test_cross_project_extractor_upsert_rolls_back_extra() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage: Arc<dyn MemoryStorage> =
            Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let mut m1 = Memory::new("memory 1".to_string(), MemoryType::Knowledge);
        m1.tags = vec!["rust".to_string()];
        m1.project_id = Some(uuid::Uuid::new_v4());
        storage.save(&m1).await.unwrap();

        // 第二个项目也带 rust 标签，形成跨项目共性主题
        let mut m2 = Memory::new("memory 2".to_string(), MemoryType::Knowledge);
        m2.tags = vec!["rust".to_string()];
        m2.project_id = Some(uuid::Uuid::new_v4());
        storage.save(&m2).await.unwrap();

        // 预置 3 条历史遗留的 cross-project 摘要（模拟旧版本产生的垃圾）
        for i in 0..3 {
            let mut stale = Memory::new(format!("stale summary {}", i), MemoryType::Knowledge);
            stale.tags = vec!["cross-project".to_string()];
            stale.importance = 0.8;
            stale.created_at = chrono::Utc::now() - chrono::Duration::hours(i as i64 + 1);
            storage.save(&stale).await.unwrap();
        }

        let extractor = CrossProjectExtractor::new(storage.clone(), 100, 10);
        let result = extractor.execute().await;

        assert!(result.success);
        // 已存在旧摘要：更新内容而非新增（created=0），多余旧条目被回滚删除
        assert_eq!(result.created_count, 0);
        let summary = storage.list_by_tag("cross-project", 100).await.unwrap();
        assert_eq!(summary.len(), 1);
        assert!(summary[0]
            .content
            .starts_with("Cross-project common themes: rust"));
        assert_eq!(summary[0].source, mr_common::types::MemorySource::System);
        assert_eq!(summary[0].importance, 0.3);
    }
}
