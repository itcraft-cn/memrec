//! # 传播增量存储
//!
//! 基于 RocksDB 实现 [`PropagationStorage`] trait，提供传播增量的持久化存储。
//!
//! ## 列族使用
//!
//! | 列族 | Key | Value | 说明 |
//! |------|-----|-------|------|
//! | `propagation_deltas` | `delta_id` | JSON | 增量主表 |
//! | `pending_propagations` | `target_id:priority:delta_id` | `delta_id` | 待应用增量队列 |
//! | `applied_propagations` | `source_id:target_id` | `delta_id` | 已应用增量索引 |

use anyhow::{Context, Result};
use async_trait::async_trait;
use memrec_common::PropagationDelta;
use std::sync::Arc;
use uuid::Uuid;

use crate::storage::{PropagationStorage, RocksDBStore};

pub struct PropagationStore {
    rocksdb: Arc<RocksDBStore>,
}

impl PropagationStore {
    pub fn new(rocksdb: Arc<RocksDBStore>) -> Self {
        Self { rocksdb }
    }

    fn make_pending_key(target_id: &Uuid, priority: u8, delta_id: &Uuid) -> String {
        format!(
            "{}:{:02}:{}",
            hyphenate(target_id),
            priority,
            hyphenate(delta_id)
        )
    }

    fn make_applied_key(source_id: &Uuid, target_id: &Uuid) -> String {
        format!("{}:{}", hyphenate(source_id), hyphenate(target_id))
    }
}

fn hyphenate(id: &Uuid) -> String {
    id.hyphenated().to_string()
}

#[async_trait]
impl PropagationStorage for PropagationStore {
    async fn save(&self, delta: &PropagationDelta) -> Result<()> {
        let cf = self.rocksdb.cf_propagation_deltas()?;
        let key = hyphenate(&delta.id);
        let value = serde_json::to_vec(delta).context("Failed to serialize delta")?;
        self.rocksdb.put_cf(cf, key.as_bytes(), &value)?;

        if !delta.applied {
            let cf_pending = self.rocksdb.cf_pending_propagations()?;
            let pending_key = Self::make_pending_key(
                &delta.target_id,
                delta.delta_type.priority(),
                &delta.id,
            );
            self.rocksdb
                .put_cf(cf_pending, pending_key.as_bytes(), key.as_bytes())?;
        }

        Ok(())
    }

    async fn get(&self, id: &Uuid) -> Result<Option<PropagationDelta>> {
        let cf = self.rocksdb.cf_propagation_deltas()?;
        let key = hyphenate(id);
        let value = self.rocksdb.get_cf(cf, key.as_bytes())?;

        match value {
            Some(bytes) => {
                let delta: PropagationDelta =
                    serde_json::from_slice(&bytes).context("Failed to deserialize delta")?;
                Ok(Some(delta))
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let delta = self.get(id).await?;
        if let Some(delta) = delta {
            let cf = self.rocksdb.cf_propagation_deltas()?;
            let key = hyphenate(id);
            self.rocksdb.delete_cf(cf, key.as_bytes())?;

            if !delta.applied {
                let cf_pending = self.rocksdb.cf_pending_propagations()?;
                let pending_key = Self::make_pending_key(
                    &delta.target_id,
                    delta.delta_type.priority(),
                    &delta.id,
                );
                self.rocksdb.delete_cf(cf_pending, pending_key.as_bytes())?;
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn list_pending(&self, target_id: &Uuid) -> Result<Vec<PropagationDelta>> {
        let cf_pending = self.rocksdb.cf_pending_propagations()?;
        let prefix = format!("{}:", hyphenate(target_id));
        let mut iter = self.rocksdb.iter_cf(cf_pending);
        iter.seek(prefix.as_bytes());

        let mut deltas = Vec::new();
        while iter.valid() {
            if let Some(key) = iter.key() {
                let key_str = String::from_utf8_lossy(key);
                if !key_str.starts_with(&prefix) {
                    break;
                }
                if let Some(value) = iter.value() {
                    let delta_id_str = String::from_utf8_lossy(value);
                    if let Ok(delta_id) = Uuid::parse_str(&delta_id_str) {
                        if let Some(delta) = self.get(&delta_id).await? {
                            deltas.push(delta);
                        }
                    }
                }
            }
            iter.next();
        }

        deltas.sort_by(|a, b| {
            b.delta_type
                .priority()
                .cmp(&a.delta_type.priority())
                .then_with(|| b.confidence.partial_cmp(&a.confidence).unwrap())
        });

        Ok(deltas)
    }

    async fn list_by_source(&self, source_id: &Uuid) -> Result<Vec<PropagationDelta>> {
        let cf = self.rocksdb.cf_propagation_deltas()?;
        let mut iter = self.rocksdb.iter_cf(cf);
        iter.seek_to_first();

        let mut deltas = Vec::new();
        while iter.valid() {
            if let Some(value) = iter.value() {
                if let Ok(delta) = serde_json::from_slice::<PropagationDelta>(value) {
                    if delta.source_id == *source_id {
                        deltas.push(delta);
                    }
                }
            }
            iter.next();
        }

        Ok(deltas)
    }

    async fn mark_applied(&self, id: &Uuid) -> Result<bool> {
        let mut delta = match self.get(id).await? {
            Some(d) => d,
            None => return Ok(false),
        };

        if delta.applied {
            return Ok(true);
        }

        let cf_pending = self.rocksdb.cf_pending_propagations()?;
        let pending_key = Self::make_pending_key(&delta.target_id, delta.delta_type.priority(), id);
        self.rocksdb.delete_cf(cf_pending, pending_key.as_bytes())?;

        delta.applied = true;
        delta.applied_at = Some(chrono::Utc::now());

        let cf = self.rocksdb.cf_propagation_deltas()?;
        let key = hyphenate(id);
        let value = serde_json::to_vec(&delta).context("Failed to serialize delta")?;
        self.rocksdb.put_cf(cf, key.as_bytes(), &value)?;

        let cf_applied = self.rocksdb.cf_applied_propagations()?;
        let applied_key = Self::make_applied_key(&delta.source_id, &delta.target_id);
        self.rocksdb
            .put_cf(cf_applied, applied_key.as_bytes(), key.as_bytes())?;

        Ok(true)
    }

    async fn has_applied(&self, source_id: &Uuid, target_id: &Uuid) -> Result<bool> {
        let cf = self.rocksdb.cf_applied_propagations()?;
        let key = Self::make_applied_key(source_id, target_id);
        let value = self.rocksdb.get_cf(cf, key.as_bytes())?;
        Ok(value.is_some())
    }

    async fn count_pending(&self) -> Result<usize> {
        let cf = self.rocksdb.cf_pending_propagations()?;
        let mut iter = self.rocksdb.iter_cf(cf);
        iter.seek_to_first();

        let mut count = 0;
        while iter.valid() {
            count += 1;
            iter.next();
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memrec_common::DeltaType;
    use tempfile::tempdir;

    async fn create_test_store() -> (PropagationStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let rocksdb = Arc::new(RocksDBStore::open(dir.path()).unwrap());
        let store = PropagationStore::new(rocksdb);
        (store, dir)
    }

    #[tokio::test]
    async fn test_save_and_get() {
        let (store, _dir) = create_test_store().await;

        let delta = PropagationDelta::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            DeltaType::KnowledgeUpdate,
            "test content".to_string(),
            0.85,
        );
        let id = delta.id;

        store.save(&delta).await.unwrap();
        let retrieved = store.get(&id).await.unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.delta_type, DeltaType::KnowledgeUpdate);
        assert_eq!(retrieved.confidence, 0.85);
    }

    #[tokio::test]
    async fn test_delete() {
        let (store, _dir) = create_test_store().await;

        let delta = PropagationDelta::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            DeltaType::Refinement,
            "test".to_string(),
            0.9,
        );
        let id = delta.id;

        store.save(&delta).await.unwrap();
        assert!(store.delete(&id).await.unwrap());
        assert!(store.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_list_pending() {
        let (store, _dir) = create_test_store().await;
        let target = Uuid::new_v4();

        let delta1 = PropagationDelta::new(
            Uuid::new_v4(),
            target,
            DeltaType::Refinement,
            "low priority".to_string(),
            0.7,
        );
        let delta2 = PropagationDelta::new(
            Uuid::new_v4(),
            target,
            DeltaType::Contradiction,
            "high priority".to_string(),
            0.9,
        );

        store.save(&delta1).await.unwrap();
        store.save(&delta2).await.unwrap();

        let pending = store.list_pending(&target).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].delta_type, DeltaType::Contradiction);
        assert_eq!(pending[1].delta_type, DeltaType::Refinement);
    }

    #[tokio::test]
    async fn test_mark_applied() {
        let (store, _dir) = create_test_store().await;

        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let delta = PropagationDelta::new(
            source,
            target,
            DeltaType::KnowledgeUpdate,
            "test".to_string(),
            0.8,
        );
        let id = delta.id;

        store.save(&delta).await.unwrap();
        assert!(!store.has_applied(&source, &target).await.unwrap());

        assert!(store.mark_applied(&id).await.unwrap());

        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert!(retrieved.applied);
        assert!(retrieved.applied_at.is_some());

        assert!(store.has_applied(&source, &target).await.unwrap());

        let pending = store.list_pending(&target).await.unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[tokio::test]
    async fn test_count_pending() {
        let (store, _dir) = create_test_store().await;

        for i in 0..5 {
            let delta = PropagationDelta::new(
                Uuid::new_v4(),
                Uuid::new_v4(),
                DeltaType::KnowledgeUpdate,
                format!("delta {}", i),
                0.8,
            );
            store.save(&delta).await.unwrap();
        }

        assert_eq!(store.count_pending().await.unwrap(), 5);
    }
}
