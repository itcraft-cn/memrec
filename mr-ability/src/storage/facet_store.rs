//! # Facet 存储实现
//!
//! [`FacetStore`] 基于 [`RocksDBStore`] 实现 [`FacetStorage`] trait，
//! 提供 Facet 的完整生命周期管理。
//!
//! ## 存储结构
//!
//! - **facets 列族**：UUID → JSON 序列化的 `MemoryFacet`
//! - **memory_facets 列族**：`memory_id:facet_id` → UUID（记忆→Facet 索引）
//!
//! ## 向量检索
//!
//! Facet 向量存储在独立的向量索引中，支持主题级别的语义检索。

use super::rocksdb::RocksDBStore;
use super::traits::{FacetSearchHit, FacetStorage, VectorPayload, VectorStorage};
use crate::storage::vector_store::VectorStore;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mr_common::MemoryFacet;
use uuid::Uuid;

pub struct FacetStore {
    rocksdb: RocksDBStore,
    vector_store: VectorStore,
}

impl FacetStore {
    pub fn new(rocksdb: RocksDBStore, vector_store: VectorStore) -> Self {
        Self {
            rocksdb,
            vector_store,
        }
    }

    fn facet_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }

    fn memory_facet_key(memory_id: &Uuid, facet_id: &Uuid) -> Vec<u8> {
        format!("{}:{}", memory_id, facet_id).into_bytes()
    }

    fn serialize_facet(facet: &MemoryFacet) -> Result<Vec<u8>> {
        serde_json::to_vec(facet).context("Failed to serialize facet")
    }

    fn deserialize_facet(data: &[u8]) -> Result<MemoryFacet> {
        serde_json::from_slice(data).context("Failed to deserialize facet")
    }
}

#[async_trait]
impl FacetStorage for FacetStore {
    async fn save(&self, facet: &MemoryFacet) -> Result<()> {
        let id_key = Self::facet_key(&facet.id);
        let data = Self::serialize_facet(facet)?;

        let cf_facets = self.rocksdb.cf_facets()?;
        self.rocksdb.put_cf(cf_facets, &id_key, &data)?;

        let memory_facet_key = Self::memory_facet_key(&facet.memory_id, &facet.id);
        let cf_memory_facets = self.rocksdb.cf_memory_facets()?;
        self.rocksdb
            .put_cf(cf_memory_facets, &memory_facet_key, &id_key)?;

        if let Some(ref embedding) = facet.embedding {
            let payload = VectorPayload {
                project_id: None,
                memory_type: "facet".to_string(),
                tags: facet.keywords.clone(),
                content_preview: facet.theme.clone(),
                importance: facet.confidence,
                chunk_group_id: None,
                chunk_index: None,
                chunk_total: None,
            };
            self.vector_store.add(&facet.id, embedding, payload).await?;
        }

        Ok(())
    }

    async fn get(&self, id: &Uuid) -> Result<Option<MemoryFacet>> {
        let id_key = Self::facet_key(id);
        let cf_facets = self.rocksdb.cf_facets()?;

        match self.rocksdb.get_cf(cf_facets, &id_key)? {
            Some(bytes) => {
                let facet = Self::deserialize_facet(&bytes)?;
                Ok(Some(facet))
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let facet = self.get(id).await?;

        match facet {
            Some(f) => {
                let id_key = Self::facet_key(id);
                let cf_facets = self.rocksdb.cf_facets()?;
                self.rocksdb.delete_cf(cf_facets, &id_key)?;

                let memory_facet_key = Self::memory_facet_key(&f.memory_id, id);
                let cf_memory_facets = self.rocksdb.cf_memory_facets()?;
                self.rocksdb
                    .delete_cf(cf_memory_facets, &memory_facet_key)?;

                self.vector_store.remove(id).await?;

                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn list_by_memory(&self, memory_id: &Uuid) -> Result<Vec<MemoryFacet>> {
        let cf_memory_facets = self.rocksdb.cf_memory_facets()?;
        let mut iter = self.rocksdb.iter_cf(cf_memory_facets);

        let prefix = format!("{}:", memory_id).into_bytes();
        let mut facets = Vec::new();

        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                if key.starts_with(&prefix) {
                    if let Some(value) = iter.value() {
                        let id_str = String::from_utf8_lossy(value);
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            if let Some(facet) = self.get(&id).await? {
                                facets.push(facet);
                            }
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(facets)
    }

    async fn count(&self) -> Result<usize> {
        let cf_facets = self.rocksdb.cf_facets()?;
        let mut iter = self.rocksdb.iter_cf(cf_facets);

        let mut count = 0;
        iter.seek_to_first();
        while iter.valid() {
            if iter.value().is_some() {
                count += 1;
            }
            iter.next();
        }

        Ok(count)
    }

    async fn search_by_theme(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<FacetSearchHit>> {
        let filter = super::traits::SearchFilter {
            project_id: None,
            include_global: true,
            memory_type: Some("facet".to_string()),
            min_score,
        };

        let hits = self
            .vector_store
            .search(query_embedding, filter, top_k)
            .await?;

        let mut facet_hits = Vec::new();
        for hit in hits {
            if let Some(facet) = self.get(&hit.memory_id).await? {
                facet_hits.push(FacetSearchHit {
                    facet_id: facet.id,
                    memory_id: facet.memory_id,
                    score: hit.score,
                    theme: facet.theme,
                    confidence: facet.confidence,
                    keywords: facet.keywords,
                });
            }
        }

        Ok(facet_hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_store() -> FacetStore {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let vector_store = VectorStore::new_in_memory();
        FacetStore::new(rocksdb, vector_store)
    }

    #[tokio::test]
    async fn test_facet_save_and_get() {
        let store = create_test_store().await;

        let memory_id = Uuid::new_v4();
        let facet = MemoryFacet::new(
            memory_id,
            "test theme".to_string(),
            0.85,
            vec!["kw1".to_string()],
        );

        store.save(&facet).await.unwrap();

        let retrieved = store.get(&facet.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().theme, "test theme");
    }

    #[tokio::test]
    async fn test_facet_list_by_memory() {
        let store = create_test_store().await;

        let memory_id = Uuid::new_v4();
        let facet1 = MemoryFacet::new(memory_id, "theme1".to_string(), 0.8, vec![]);
        let facet2 = MemoryFacet::new(memory_id, "theme2".to_string(), 0.9, vec![]);

        store.save(&facet1).await.unwrap();
        store.save(&facet2).await.unwrap();

        let facets = store.list_by_memory(&memory_id).await.unwrap();
        assert_eq!(facets.len(), 2);
    }

    #[tokio::test]
    async fn test_facet_delete() {
        let store = create_test_store().await;

        let facet = MemoryFacet::new(Uuid::new_v4(), "test".to_string(), 0.8, vec![]);
        store.save(&facet).await.unwrap();

        let deleted = store.delete(&facet.id).await.unwrap();
        assert!(deleted);

        let retrieved = store.get(&facet.id).await.unwrap();
        assert!(retrieved.is_none());
    }
}
