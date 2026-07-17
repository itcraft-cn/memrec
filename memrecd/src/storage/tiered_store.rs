//! # 分层存储实现
//!
//! 实现记忆摘要和关键词的 RocksDB 存储。
//!
//! ## 存储结构
//!
//! | Column Family | Key | Value | 说明 |
//! |---------------|-----|-------|------|
//! | `memory_summaries` | memory_id (UUID bytes) | String (UTF-8) | 预计算摘要 |
//! | `memory_keywords` | memory_id (UUID bytes) | JSON Array | 预计算关键词 |

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::storage::traits::TieredStorage;
use crate::storage::RocksDBStore;

pub struct TieredStore {
    rocksdb: RocksDBStore,
}

impl TieredStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        TieredStore { rocksdb }
    }

    fn memory_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }
}

#[async_trait]
impl TieredStorage for TieredStore {
    async fn save_summary(&self, memory_id: &Uuid, summary: &str) -> Result<()> {
        let key = Self::memory_key(memory_id);
        let cf = self.rocksdb.cf_memory_summaries()?;
        self.rocksdb.put_cf(cf, &key, summary.as_bytes())?;
        Ok(())
    }

    async fn get_summary(&self, memory_id: &Uuid) -> Result<Option<String>> {
        let key = Self::memory_key(memory_id);
        let cf = self.rocksdb.cf_memory_summaries()?;
        match self.rocksdb.get_cf(cf, &key)? {
            Some(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            None => Ok(None),
        }
    }

    async fn delete_summary(&self, memory_id: &Uuid) -> Result<bool> {
        let key = Self::memory_key(memory_id);
        let cf = self.rocksdb.cf_memory_summaries()?;
        let exists = self.rocksdb.get_cf(cf, &key)?.is_some();
        if exists {
            self.rocksdb.delete_cf(cf, &key)?;
        }
        Ok(exists)
    }

    async fn save_keywords(&self, memory_id: &Uuid, keywords: &[String]) -> Result<()> {
        let key = Self::memory_key(memory_id);
        let value = serde_json::to_vec(keywords)?;
        let cf = self.rocksdb.cf_memory_keywords()?;
        self.rocksdb.put_cf(cf, &key, &value)?;
        Ok(())
    }

    async fn get_keywords(&self, memory_id: &Uuid) -> Result<Option<Vec<String>>> {
        let key = Self::memory_key(memory_id);
        let cf = self.rocksdb.cf_memory_keywords()?;
        match self.rocksdb.get_cf(cf, &key)? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    async fn delete_keywords(&self, memory_id: &Uuid) -> Result<bool> {
        let key = Self::memory_key(memory_id);
        let cf = self.rocksdb.cf_memory_keywords()?;
        let exists = self.rocksdb.get_cf(cf, &key)?.is_some();
        if exists {
            self.rocksdb.delete_cf(cf, &key)?;
        }
        Ok(exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_store() -> TieredStore {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        TieredStore::new(rocksdb)
    }

    #[tokio::test]
    async fn test_save_and_get_summary() {
        let store = create_test_store().await;
        let id = Uuid::new_v4();

        store.save_summary(&id, "test summary").await.unwrap();
        let summary = store.get_summary(&id).await.unwrap();
        assert_eq!(summary, Some("test summary".to_string()));
    }

    #[tokio::test]
    async fn test_get_nonexistent_summary() {
        let store = create_test_store().await;
        let id = Uuid::new_v4();

        let summary = store.get_summary(&id).await.unwrap();
        assert_eq!(summary, None);
    }

    #[tokio::test]
    async fn test_delete_summary() {
        let store = create_test_store().await;
        let id = Uuid::new_v4();

        store.save_summary(&id, "test").await.unwrap();
        let deleted = store.delete_summary(&id).await.unwrap();
        assert!(deleted);

        let deleted_again = store.delete_summary(&id).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_save_and_get_keywords() {
        let store = create_test_store().await;
        let id = Uuid::new_v4();

        let keywords = vec!["rust".to_string(), "async".to_string()];
        store.save_keywords(&id, &keywords).await.unwrap();

        let retrieved = store.get_keywords(&id).await.unwrap();
        assert_eq!(retrieved, Some(keywords));
    }

    #[tokio::test]
    async fn test_get_nonexistent_keywords() {
        let store = create_test_store().await;
        let id = Uuid::new_v4();

        let keywords = store.get_keywords(&id).await.unwrap();
        assert_eq!(keywords, None);
    }

    #[tokio::test]
    async fn test_delete_keywords() {
        let store = create_test_store().await;
        let id = Uuid::new_v4();

        store
            .save_keywords(&id, &["test".to_string()])
            .await
            .unwrap();
        let deleted = store.delete_keywords(&id).await.unwrap();
        assert!(deleted);

        let deleted_again = store.delete_keywords(&id).await.unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_save_summaries_batch() {
        let store = create_test_store().await;
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        store
            .save_summaries_batch(&[(id1, "summary1".to_string()), (id2, "summary2".to_string())])
            .await
            .unwrap();

        assert_eq!(
            store.get_summary(&id1).await.unwrap(),
            Some("summary1".to_string())
        );
        assert_eq!(
            store.get_summary(&id2).await.unwrap(),
            Some("summary2".to_string())
        );
    }

    #[tokio::test]
    async fn test_save_keywords_batch() {
        let store = create_test_store().await;
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        store
            .save_keywords_batch(&[
                (id1, vec!["a".to_string()]),
                (id2, vec!["b".to_string()]),
            ])
            .await
            .unwrap();

        assert_eq!(
            store.get_keywords(&id1).await.unwrap(),
            Some(vec!["a".to_string()])
        );
        assert_eq!(
            store.get_keywords(&id2).await.unwrap(),
            Some(vec!["b".to_string()])
        );
    }
}
