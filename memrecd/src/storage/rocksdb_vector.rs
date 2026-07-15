use super::traits::{SearchFilter, SearchHit, VectorPayload, VectorStorage};
use anyhow::Result;
use async_trait::async_trait;
use rocksdb::{Options, WriteBatch, DB};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const CF_VECTORS: &str = "vectors";
const CF_PAYLOADS: &str = "payloads";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredVector {
    embedding: Vec<f32>,
    payload: VectorPayload,
}

pub struct RocksDBVectorStore {
    db: Arc<DB>,
    dimension: usize,
    cache: Arc<Mutex<HashMap<Uuid, StoredVector>>>,
    dirty: Arc<Mutex<bool>>,
}

impl RocksDBVectorStore {
    pub fn open(path: &std::path::Path, dimension: usize) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cf_vectors = rocksdb::ColumnFamilyDescriptor::new(CF_VECTORS, Options::default());
        let cf_payloads = rocksdb::ColumnFamilyDescriptor::new(CF_PAYLOADS, Options::default());

        let db = DB::open_cf_descriptors(&opts, path, vec![cf_vectors, cf_payloads])?;

        let cache = Arc::new(Mutex::new(HashMap::new()));
        let db = Arc::new(db);

        {
            let cf = db.cf_handle(CF_VECTORS).expect("CF_VECTORS not found");
            let mut cache_lock = cache.lock().unwrap();

            for (key, value) in db.iterator_cf(&cf, rocksdb::IteratorMode::Start).flatten() {
                if let Ok(id_str) = std::str::from_utf8(&key) {
                    if let Ok(id) = Uuid::parse_str(id_str) {
                        if let Ok(stored) = serde_json::from_slice::<StoredVector>(&value) {
                            cache_lock.insert(id, stored);
                        }
                    }
                }
            }
        }

        Ok(Self {
            db,
            dimension,
            cache,
            dirty: Arc::new(Mutex::new(false)),
        })
    }

    pub fn save(&self) -> Result<()> {
        let dirty = *self.dirty.lock().unwrap();
        if !dirty {
            return Ok(());
        }

        let cache = self.cache.lock().unwrap();
        let cf_vectors = self.db.cf_handle(CF_VECTORS).expect("CF_VECTORS not found");

        let mut batch = WriteBatch::default();

        for (id, stored) in cache.iter() {
            let key = id.to_string();
            let value = serde_json::to_vec(stored)?;
            batch.put_cf(&cf_vectors, key.as_bytes(), &value);
        }

        self.db.write(batch)?;

        *self.dirty.lock().unwrap() = false;

        Ok(())
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
        let norm_a = (a.iter().map(|x| x * x).sum::<f32>()).sqrt();
        let norm_b = (b.iter().map(|x| x * x).sum::<f32>()).sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    pub fn count_cached(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

#[async_trait]
impl VectorStorage for RocksDBVectorStore {
    async fn add(&self, id: &Uuid, embedding: &[f32], payload: VectorPayload) -> Result<()> {
        if embedding.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            ));
        }

        let stored = StoredVector {
            embedding: embedding.to_vec(),
            payload,
        };

        self.cache.lock().unwrap().insert(*id, stored);
        *self.dirty.lock().unwrap() = true;

        Ok(())
    }

    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let existed = self.cache.lock().unwrap().remove(id).is_some();
        if existed {
            *self.dirty.lock().unwrap() = true;

            let cf_vectors = self.db.cf_handle(CF_VECTORS).expect("CF_VECTORS not found");
            self.db.delete_cf(&cf_vectors, id.to_string().as_bytes())?;
        }
        Ok(existed)
    }

    async fn search(
        &self,
        query: &[f32],
        filter: SearchFilter,
        top_k: usize,
    ) -> Result<Vec<SearchHit>> {
        if query.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Query dimension mismatch: expected {}, got {}",
                self.dimension,
                query.len()
            ));
        }

        let cache = self.cache.lock().unwrap();

        let mut similarities: Vec<(Uuid, f32, &StoredVector)> = cache
            .iter()
            .filter_map(|(id, stored)| {
                if filter.project_id.is_some()
                    && stored.payload.project_id != filter.project_id
                    && (!filter.include_global || stored.payload.project_id != Some(Uuid::nil()))
                {
                    return None;
                }

                if filter.memory_type.is_some()
                    && Some(stored.payload.memory_type.as_str()) != filter.memory_type.as_deref()
                {
                    return None;
                }

                let sim = Self::cosine_similarity(query, &stored.embedding);
                if sim < filter.min_score {
                    return None;
                }

                Some((*id, sim, stored))
            })
            .collect();

        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        similarities.truncate(top_k);

        let hits = similarities
            .into_iter()
            .map(|(id, score, stored)| SearchHit {
                memory_id: id,
                score,
                payload: stored.payload.clone(),
            })
            .collect();

        Ok(hits)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>> {
        Ok(self
            .cache
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.embedding.clone()))
    }

    async fn count(&self) -> Result<usize> {
        Ok(self.cache.lock().unwrap().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_rocksdb_vector_store() {
        let dir = tempdir().unwrap();
        let id = Uuid::new_v4();
        let embedding = vec![1.0, 2.0, 3.0];
        let payload = VectorPayload {
            content_preview: "test".to_string(),
            ..Default::default()
        };

        {
            let store = RocksDBVectorStore::open(dir.path(), 3).unwrap();
            store.add(&id, &embedding, payload).await.unwrap();

            let retrieved = store.get(&id).await.unwrap();
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap(), embedding);

            store.save().unwrap();
        }

        let store2 = RocksDBVectorStore::open(dir.path(), 3).unwrap();
        let retrieved2 = store2.get(&id).await.unwrap();
        assert!(retrieved2.is_some());
        assert_eq!(retrieved2.unwrap(), embedding);
    }
}
