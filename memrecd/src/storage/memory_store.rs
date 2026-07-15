use super::rocksdb::RocksDBStore;
use super::traits::MemoryStorage;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use memrec_common::{Memory, MemoryType};
use uuid::Uuid;

pub struct MemoryStore {
    rocksdb: RocksDBStore,
}

impl MemoryStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }

    fn memory_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }

    fn tag_key(tag: &str, id: &Uuid) -> Vec<u8> {
        format!("{}:{}", tag, id).into_bytes()
    }

    fn serialize_memory(memory: &Memory) -> Result<Vec<u8>> {
        serde_json::to_vec(memory).context("Failed to serialize memory")
    }

    fn deserialize_memory(data: &[u8]) -> Result<Memory> {
        serde_json::from_slice(data).context("Failed to deserialize memory")
    }

    fn collect_memories_from_iter(
        mut iter: rocksdb::DBRawIterator<'_>,
        limit: usize,
        filter_deleted: bool,
    ) -> Vec<Memory> {
        let mut memories = Vec::new();
        let mut count = 0;

        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(value) = iter.value() {
                if let Ok(memory) = Self::deserialize_memory(value) {
                    if !filter_deleted || !memory.is_deleted {
                        memories.push(memory);
                        count += 1;
                    }
                }
            }
            iter.next();
        }

        memories
    }
}

#[async_trait]
impl MemoryStorage for MemoryStore {
    async fn save(&self, memory: &Memory) -> Result<()> {
        let id_key = Self::memory_key(&memory.id);
        let data = Self::serialize_memory(memory)?;

        let cf_memories = self.rocksdb.cf_memories()?;
        self.rocksdb.put_cf(cf_memories, &id_key, &data)?;

        for tag in &memory.tags {
            let tag_key = Self::tag_key(tag, &memory.id);
            let cf_by_tag = self.rocksdb.cf_by_tag()?;
            self.rocksdb.put_cf(cf_by_tag, &tag_key, &id_key)?;
        }

        let importance_key = Self::memory_key(&memory.id);
        let importance_data = memory.importance.to_string().into_bytes();
        let cf_importance = self.rocksdb.cf_importance()?;
        self.rocksdb
            .put_cf(cf_importance, &importance_key, &importance_data)?;

        Ok(())
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Memory>> {
        let id_key = Self::memory_key(id);
        let cf_memories = self.rocksdb.cf_memories()?;

        match self.rocksdb.get_cf(cf_memories, &id_key)? {
            Some(bytes) => {
                let memory = Self::deserialize_memory(&bytes)?;
                Ok(Some(memory))
            }
            None => Ok(None),
        }
    }

    async fn update(&self, memory: &Memory) -> Result<()> {
        self.save(memory).await
    }

    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let memory = self.get(id).await?;

        match memory {
            Some(mem) => {
                if mem.is_deleted {
                    let id_key = Self::memory_key(id);
                    let cf_memories = self.rocksdb.cf_memories()?;
                    self.rocksdb.delete_cf(cf_memories, &id_key)?;

                    let cf_deleted = self.rocksdb.cf_deleted()?;
                    self.rocksdb.delete_cf(cf_deleted, &id_key)?;

                    for tag in &mem.tags {
                        let tag_key = Self::tag_key(tag, id);
                        let cf_by_tag = self.rocksdb.cf_by_tag()?;
                        self.rocksdb.delete_cf(cf_by_tag, &tag_key)?;
                    }

                    Ok(true)
                } else {
                    let mut mem_deleted = mem;
                    mem_deleted.is_deleted = true;
                    mem_deleted.deleted_at = Some(Utc::now());
                    self.update(&mem_deleted).await?;

                    let id_key = Self::memory_key(id);
                    let cf_deleted = self.rocksdb.cf_deleted()?;
                    self.rocksdb.put_cf(cf_deleted, &id_key, &id_key)?;

                    Ok(false)
                }
            }
            None => Ok(false),
        }
    }

    async fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        let cf_memories = self.rocksdb.cf_memories()?;
        let iter = self.rocksdb.iter_cf(cf_memories);

        Ok(Self::collect_memories_from_iter(iter, limit, true))
    }

    async fn list_by_type(&self, memory_type: MemoryType, limit: usize) -> Result<Vec<Memory>> {
        let all = self.list(limit * 10).await?;
        Ok(all
            .into_iter()
            .filter(|m| m.memory_type == memory_type)
            .take(limit)
            .collect())
    }

    async fn list_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Memory>> {
        let cf_by_tag = self.rocksdb.cf_by_tag()?;
        let mut iter = self.rocksdb.iter_cf(cf_by_tag);

        let prefix = format!("{}:", tag).into_bytes();
        let mut memories = Vec::new();
        let mut count = 0;

        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(key) = iter.key() {
                if key.starts_with(&prefix) {
                    if let Some(value) = iter.value() {
                        let id_str = String::from_utf8_lossy(value);
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            if let Some(memory) = self.get(&id).await? {
                                if !memory.is_deleted {
                                    memories.push(memory);
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }

    async fn list_by_importance(&self, min: f32, max: f32) -> Result<Vec<Memory>> {
        let cf_importance = self.rocksdb.cf_importance()?;
        let mut iter = self.rocksdb.iter_cf(cf_importance);

        let mut memories = Vec::new();

        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                if let Some(value) = iter.value() {
                    let importance_str = String::from_utf8_lossy(value);
                    if let Ok(importance) = importance_str.parse::<f32>() {
                        if importance >= min && importance <= max {
                            let id_str = String::from_utf8_lossy(key);
                            if let Ok(id) = Uuid::parse_str(&id_str) {
                                if let Some(memory) = self.get(&id).await? {
                                    if !memory.is_deleted {
                                        memories.push(memory);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }

    async fn list_deleted(&self) -> Result<Vec<Memory>> {
        let cf_deleted = self.rocksdb.cf_deleted()?;
        let mut iter = self.rocksdb.iter_cf(cf_deleted);

        let mut memories = Vec::new();

        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                let id_str = String::from_utf8_lossy(key);
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    if let Some(memory) = self.get(&id).await? {
                        if memory.is_deleted {
                            memories.push(memory);
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }

    async fn count(&self) -> Result<usize> {
        let memories = self.list(10000).await?;
        Ok(memories.len())
    }

    async fn count_deleted(&self) -> Result<usize> {
        let deleted = self.list_deleted().await?;
        Ok(deleted.len())
    }

    async fn list_by_project(&self, project_id: &Uuid) -> Result<Vec<Memory>> {
        let all = self.list(10000).await?;
        Ok(all
            .into_iter()
            .filter(|m| m.project_id == Some(*project_id))
            .collect())
    }

    async fn get_chunks_by_group(&self, chunk_group_id: &Uuid) -> Result<Vec<Memory>> {
        let all = self.list(10000).await?;
        Ok(all
            .into_iter()
            .filter(|m| m.chunk_group_id == Some(*chunk_group_id))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_memory_save_and_get() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);

        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["tag1".to_string()]);

        store.save(&memory).await.unwrap();

        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "test content");
    }

    #[tokio::test]
    async fn test_memory_list() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);

        for i in 0..5 {
            let memory = Memory::new(format!("content {}", i), MemoryType::Conversation);
            store.save(&memory).await.unwrap();
        }

        let memories = store.list(10).await.unwrap();
        assert_eq!(memories.len(), 5);
    }

    #[tokio::test]
    async fn test_memory_list_by_tag() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);

        let m1 = Memory::new("content 1".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["rust".to_string()]);
        let m2 = Memory::new("content 2".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["python".to_string()]);

        store.save(&m1).await.unwrap();
        store.save(&m2).await.unwrap();

        let rust_memories = store.list_by_tag("rust", 10).await.unwrap();
        assert_eq!(rust_memories.len(), 1);
    }

    #[tokio::test]
    async fn test_memory_soft_delete() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);

        let memory = Memory::new("test".to_string(), MemoryType::Conversation);
        store.save(&memory).await.unwrap();

        let deleted = store.delete(&memory.id).await.unwrap();
        assert!(!deleted);

        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert!(retrieved.unwrap().is_deleted);
    }
}
