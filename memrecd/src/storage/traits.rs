use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use memrec_common::{Memory, MemoryType};
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait MemoryStorage: Send + Sync {
    async fn save(&self, memory: &Memory) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<Memory>>;
    async fn update(&self, memory: &Memory) -> Result<()>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;
    
    async fn list(&self, limit: usize) -> Result<Vec<Memory>>;
    async fn list_by_type(&self, memory_type: MemoryType, limit: usize) -> Result<Vec<Memory>>;
    async fn list_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Memory>>;
    
    async fn list_by_importance(&self, min: f32, max: f32) -> Result<Vec<Memory>>;
    async fn list_deleted(&self) -> Result<Vec<Memory>>;
    
    async fn count(&self) -> Result<usize>;
    async fn count_deleted(&self) -> Result<usize>;
    
    async fn list_by_project(&self, project_id: &Uuid) -> Result<Vec<Memory>>;
    async fn get_chunks_by_group(&self, chunk_group_id: &Uuid) -> Result<Vec<Memory>>;
}

#[async_trait]
pub trait ProjectStorage: Send + Sync {
    async fn save(&self, project: &memrec_common::Project) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<memrec_common::Project>>;
    async fn get_by_name(&self, name: &str) -> Result<Option<memrec_common::Project>>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;
    
    async fn list(&self) -> Result<Vec<memrec_common::Project>>;
}

#[async_trait]
pub trait ConfigStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VectorPayload {
    pub project_id: Option<Uuid>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub content_preview: String,
    pub importance: f32,
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    pub project_id: Option<Uuid>,
    pub include_global: bool,
    pub memory_type: Option<String>,
    pub min_score: f32,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub memory_id: Uuid,
    pub score: f32,
    pub payload: VectorPayload,
}

#[async_trait]
pub trait VectorStorage: Send + Sync {
    async fn add(&self, id: &Uuid, embedding: &[f32], payload: VectorPayload) -> Result<()>;
    async fn remove(&self, id: &Uuid) -> Result<bool>;
    async fn search(&self, query: &[f32], filter: SearchFilter, top_k: usize) -> Result<Vec<SearchHit>>;
    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>>;
    async fn count(&self) -> Result<usize>;
}