//! # 生命周期管理器
//!
//! [`LifecycleManager`] 执行两类清理操作：
//!
//! 1. **软删除清理**：已软删除超过恢复天数的记忆执行硬删除
//! 2. **低重要性清理**：重要性低于阈值且长期未访问的记忆执行硬删除
//!
//! 还支持全量重要性重算，确保评分随时间推移保持准确。

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tracing::info;

use crate::importance::ImportanceCalculator;
use crate::storage::MemoryStorage;
use memrec_common::MemoryConfig;

/// 生命周期管理器，协调记忆的保留与删除策略。
pub struct LifecycleManager {
    storage: Arc<dyn MemoryStorage>,
    calculator: ImportanceCalculator,
    config: MemoryConfig,
}

impl LifecycleManager {
    /// 创建生命周期管理器。
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        calculator: ImportanceCalculator,
        config: MemoryConfig,
    ) -> Self {
        Self {
            storage,
            calculator,
            config,
        }
    }

    /// 重算所有记忆的重要性分数，更新有变化的记录。
    pub async fn recalculate_importance(&self) -> Result<()> {
        info!("Recalculating importance for all memories");

        let memories = self.storage.list(1000).await?;

        for memory in memories {
            let new_importance = self.calculator.calculate(&memory);

            if memory.importance != new_importance {
                let mut updated = memory;
                updated.importance = new_importance;
                self.storage.update(&updated).await?;
            }
        }

        info!("Importance recalculation completed");
        Ok(())
    }

    /// 执行一轮清理：先清理已软删除的记忆，再清理低重要性记忆。
    pub async fn cleanup_cycle(&self) -> Result<()> {
        info!("Starting cleanup cycle");

        self.cleanup_deleted().await?;
        self.cleanup_low_importance().await?;

        info!("Cleanup cycle completed");
        Ok(())
    }

    /// 清理已软删除且超过恢复天数的记忆（硬删除）。
    async fn cleanup_deleted(&self) -> Result<()> {
        let deleted = self.storage.list_deleted().await?;
        let now = Utc::now();

        for memory in deleted {
            if let Some(deleted_at) = memory.deleted_at {
                let days_since_delete = (now - deleted_at).num_days();

                if days_since_delete > self.config.soft_delete_recovery_days as i64 {
                    info!(
                        "Hard deleting memory {} (deleted {} days ago)",
                        memory.id, days_since_delete
                    );
                    self.storage.delete(&memory.id).await?;
                }
            }
        }

        Ok(())
    }

    /// 清理重要性低于阈值且长期未访问的记忆（硬删除）。
    async fn cleanup_low_importance(&self) -> Result<()> {
        let low_importance = self
            .storage
            .list_by_importance(0.0, self.config.hard_delete_importance)
            .await?;

        let now = Utc::now();

        for memory in low_importance {
            let days_inactive = (now - memory.last_accessed).num_days();

            if days_inactive > self.config.hard_delete_inactive_days as i64 {
                info!(
                    "Deleting low importance memory {} (inactive {} days)",
                    memory.id, days_inactive
                );
                self.storage.delete(&memory.id).await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use memrec_common::{Memory, MemoryType};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_recalculate_importance() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        storage.save(&memory).await.unwrap();

        let calc = ImportanceCalculator::default();
        let config = MemoryConfig::default();

        let manager = LifecycleManager::new(storage.clone(), calc, config);

        manager.recalculate_importance().await.unwrap();

        let retrieved = storage.get(&memory.id).await.unwrap().unwrap();
        assert!(retrieved.importance > 0.0);
    }
}
