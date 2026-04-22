use anyhow::Result;
use std::sync::Arc;
use chrono::Utc;
use tracing::info;

use memrec_common::{Memory, MemoryConfig};
use crate::storage::MemoryStorage;
use crate::importance::ImportanceCalculator;

pub struct LifecycleManager {
    storage: Arc<dyn MemoryStorage>,
    calculator: ImportanceCalculator,
    config: MemoryConfig,
}

impl LifecycleManager {
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
    
    pub async fn cleanup_cycle(&self) -> Result<()> {
        info!("Starting cleanup cycle");
        
        self.cleanup_deleted().await?;
        self.cleanup_low_importance().await?;
        
        info!("Cleanup cycle completed");
        Ok(())
    }
    
    async fn cleanup_deleted(&self) -> Result<()> {
        let deleted = self.storage.list_deleted().await?;
        let now = Utc::now();
        
        for memory in deleted {
            if let Some(deleted_at) = memory.deleted_at {
                let days_since_delete = (now - deleted_at).num_days();
                
                if days_since_delete > self.config.soft_delete_recovery_days as i64 {
                    info!("Hard deleting memory {} (deleted {} days ago)", 
                        memory.id, days_since_delete);
                    self.storage.delete(&memory.id).await?;
                }
            }
        }
        
        Ok(())
    }
    
    async fn cleanup_low_importance(&self) -> Result<()> {
        let low_importance = self.storage.list_by_importance(
            0.0,
            self.config.hard_delete_importance,
        ).await?;
        
        let now = Utc::now();
        
        for memory in low_importance {
            let days_inactive = (now - memory.last_accessed).num_days();
            
            if days_inactive > self.config.hard_delete_inactive_days as i64 {
                info!("Deleting low importance memory {} (inactive {} days)", 
                    memory.id, days_inactive);
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
    use tempfile::tempdir;
    use memrec_common::MemoryType;
    
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