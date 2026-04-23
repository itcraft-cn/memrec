use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use super::traits::{VectorStorage, VectorPayload, SearchFilter, SearchHit};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorRecord {
    id: Uuid,
    embedding: Vec<f32>,
    payload: VectorPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorData {
    dimension: usize,
    vectors: Vec<VectorRecord>,
}

pub struct PersistentVectorStore {
    dimension: usize,
    data_path: PathBuf,
    id_to_index: Arc<Mutex<HashMap<Uuid, usize>>>,
    vectors: Arc<Mutex<Vec<Vec<f32>>>>,
    payloads: Arc<Mutex<HashMap<Uuid, VectorPayload>>>,
}

impl PersistentVectorStore {
    pub fn new(dimension: usize, data_dir: &Path) -> Result<Self> {
        let data_path = data_dir.join("vectors.json");
        
        let (vectors, payloads, id_to_index) = if data_path.exists() {
            Self::load_from_file(&data_path)?
        } else {
            (Vec::new(), HashMap::new(), HashMap::new())
        };
        
        Ok(Self {
            dimension,
            data_path,
            id_to_index: Arc::new(Mutex::new(id_to_index)),
            vectors: Arc::new(Mutex::new(vectors)),
            payloads: Arc::new(Mutex::new(payloads)),
        })
    }
    
    fn load_from_file(path: &Path) -> Result<(Vec<Vec<f32>>, HashMap<Uuid, VectorPayload>, HashMap<Uuid, usize>)> {
        let data: VectorData = serde_json::from_str(&fs::read_to_string(path)?)?;
        
        let mut vectors = Vec::new();
        let mut payloads = HashMap::new();
        let mut id_to_index = HashMap::new();
        
        for (index, record) in data.vectors.into_iter().enumerate() {
            vectors.push(record.embedding);
            payloads.insert(record.id, record.payload);
            id_to_index.insert(record.id, index);
        }
        
        Ok((vectors, payloads, id_to_index))
    }
    
    pub fn save(&self) -> Result<()> {
        let vectors = self.vectors.lock().unwrap();
        let payloads = self.payloads.lock().unwrap();
        let id_to_index = self.id_to_index.lock().unwrap();
        
        let records: Vec<VectorRecord> = id_to_index.iter()
            .map(|(id, index)| {
                VectorRecord {
                    id: *id,
                    embedding: vectors[*index].clone(),
                    payload: payloads.get(id).cloned().unwrap_or_default(),
                }
            })
            .collect();
        
        let data = VectorData {
            dimension: self.dimension,
            vectors: records,
        };
        
        let json = serde_json::to_string(&data)?;
        fs::write(&self.data_path, json)?;
        
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
    
    pub fn count_in_memory(&self) -> usize {
        self.id_to_index.lock().unwrap().len()
    }
}

#[async_trait]
impl VectorStorage for PersistentVectorStore {
    async fn add(&self, id: &Uuid, embedding: &[f32], payload: VectorPayload) -> Result<()> {
        if embedding.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            ));
        }
        
        let mut vectors = self.vectors.lock().unwrap();
        let mut id_to_index = self.id_to_index.lock().unwrap();
        let mut payloads = self.payloads.lock().unwrap();
        
        if let Some(old_index) = id_to_index.get(id) {
            vectors[*old_index] = embedding.to_vec();
        } else {
            let index = vectors.len();
            vectors.push(embedding.to_vec());
            id_to_index.insert(*id, index);
        }
        payloads.insert(*id, payload);
        
        Ok(())
    }
    
    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let mut id_to_index = self.id_to_index.lock().unwrap();
        let mut payloads = self.payloads.lock().unwrap();
        
        let existed = id_to_index.remove(id).is_some();
        payloads.remove(id);
        
        Ok(existed)
    }
    
    async fn search(&self, query: &[f32], filter: SearchFilter, top_k: usize) -> Result<Vec<SearchHit>> {
        if query.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Query dimension mismatch: expected {}, got {}",
                self.dimension,
                query.len()
            ));
        }
        
        let vectors = self.vectors.lock().unwrap();
        let id_to_index = self.id_to_index.lock().unwrap();
        let payloads = self.payloads.lock().unwrap();
        
        let mut similarities: Vec<(Uuid, f32)> = id_to_index.iter()
            .filter_map(|(id, index)| {
                let payload = payloads.get(id)?;
                
                if filter.project_id.is_some() && payload.project_id != filter.project_id {
                    if !filter.include_global || payload.project_id != Some(Uuid::nil()) {
                        return None;
                    }
                }
                
                if filter.memory_type.is_some() && Some(payload.memory_type.as_str()) != filter.memory_type.as_deref() {
                    return None;
                }
                
                let sim = Self::cosine_similarity(query, &vectors[*index]);
                if sim < filter.min_score {
                    return None;
                }
                
                Some((*id, sim))
            })
            .collect();
        
        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        similarities.truncate(top_k);
        
        let hits = similarities.into_iter()
            .map(|(id, score)| SearchHit {
                memory_id: id,
                score,
                payload: payloads.get(&id).cloned().unwrap_or_default(),
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
        Ok(self.id_to_index.lock().unwrap().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_persistent_vector_store() {
        let dir = tempdir().unwrap();
        let store = PersistentVectorStore::new(3, dir.path()).unwrap();
        
        let id = Uuid::new_v4();
        let embedding = vec![1.0, 2.0, 3.0];
        let payload = VectorPayload {
            content_preview: "test".to_string(),
            ..Default::default()
        };
        
        store.add(&id, &embedding, payload).await.unwrap();
        
        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), embedding);
        
        store.save().unwrap();
        
        let store2 = PersistentVectorStore::new(3, dir.path()).unwrap();
        let retrieved2 = store2.get(&id).await.unwrap();
        assert!(retrieved2.is_some());
        assert_eq!(retrieved2.unwrap(), embedding);
    }
}