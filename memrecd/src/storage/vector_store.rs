use super::traits::{SearchFilter, SearchHit, VectorPayload, VectorStorage};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct VectorStore {
    dimension: usize,
    id_to_index: Arc<Mutex<HashMap<Uuid, usize>>>,
    index_to_id: Arc<Mutex<HashMap<usize, Uuid>>>,
    vectors: Arc<Mutex<Vec<Vec<f32>>>>,
}

impl VectorStore {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            id_to_index: Arc::new(Mutex::new(HashMap::new())),
            index_to_id: Arc::new(Mutex::new(HashMap::new())),
            vectors: Arc::new(Mutex::new(Vec::new())),
        }
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
}

#[async_trait]
impl VectorStorage for VectorStore {
    async fn add(&self, id: &Uuid, embedding: &[f32], _payload: VectorPayload) -> Result<()> {
        if embedding.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            ));
        }

        let mut vectors = self.vectors.lock().unwrap();
        let mut id_to_index = self.id_to_index.lock().unwrap();
        let mut index_to_id = self.index_to_id.lock().unwrap();

        let index = vectors.len();
        vectors.push(embedding.to_vec());
        id_to_index.insert(*id, index);
        index_to_id.insert(index, *id);

        Ok(())
    }

    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let mut id_to_index = self.id_to_index.lock().unwrap();
        let mut index_to_id = self.index_to_id.lock().unwrap();

        if let Some(index) = id_to_index.remove(id) {
            index_to_id.remove(&index);
            Ok(true)
        } else {
            Ok(false)
        }
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

        let vectors = self.vectors.lock().unwrap();
        let id_to_index = self.id_to_index.lock().unwrap();

        let mut similarities: Vec<(Uuid, f32)> = id_to_index
            .iter()
            .map(|(id, index)| {
                let sim = Self::cosine_similarity(query, &vectors[*index]);
                (*id, sim)
            })
            .filter(|(_, sim)| *sim >= filter.min_score)
            .collect();

        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        similarities.truncate(top_k);

        let hits = similarities
            .into_iter()
            .map(|(id, score)| SearchHit {
                memory_id: id,
                score,
                payload: VectorPayload::default(),
            })
            .collect();

        Ok(hits)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>> {
        let id_to_index = self.id_to_index.lock().unwrap();
        let vectors = self.vectors.lock().unwrap();

        if let Some(index) = id_to_index.get(id) {
            Ok(Some(vectors[*index].clone()))
        } else {
            Ok(None)
        }
    }

    async fn count(&self) -> Result<usize> {
        let id_to_index = self.id_to_index.lock().unwrap();
        Ok(id_to_index.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vector_add_and_get() {
        let store = VectorStore::new(3);
        let id = Uuid::new_v4();
        let embedding = vec![1.0, 2.0, 3.0];

        store
            .add(&id, &embedding, VectorPayload::default())
            .await
            .unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), embedding);
    }

    #[tokio::test]
    async fn test_vector_search() {
        let store = VectorStore::new(3);

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        store
            .add(&id1, &[1.0, 0.0, 0.0], VectorPayload::default())
            .await
            .unwrap();
        store
            .add(&id2, &[0.9, 0.1, 0.0], VectorPayload::default())
            .await
            .unwrap();
        store
            .add(&id3, &[0.0, 1.0, 0.0], VectorPayload::default())
            .await
            .unwrap();

        let filter = SearchFilter {
            min_score: 0.0,
            ..Default::default()
        };

        let results = store.search(&[1.0, 0.0, 0.0], filter, 2).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].memory_id, id1);
        assert!(results[0].score > 0.99);
    }

    #[tokio::test]
    async fn test_vector_remove() {
        let store = VectorStore::new(3);
        let id = Uuid::new_v4();

        store
            .add(&id, &[1.0, 2.0, 3.0], VectorPayload::default())
            .await
            .unwrap();

        let removed = store.remove(&id).await.unwrap();
        assert!(removed);

        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_none());
    }
}
