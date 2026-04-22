use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use memrec_common::{Memory, MemoryType};

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

#[async_trait]
pub trait VectorStorage: Send + Sync {
    async fn add(&self, id: &Uuid, embedding: &[f32]) -> Result<()>;
    async fn remove(&self, id: &Uuid) -> Result<bool>;
    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(Uuid, f32)>>;
    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>>;
    async fn count(&self) -> Result<usize>;
}