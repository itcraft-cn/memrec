pub mod fastembed;

pub use fastembed::FastEmbedGenerator;

use anyhow::Result;
use memrec_common::ModelConfig;
use std::sync::Arc;

pub trait EmbeddingGenerator: Send + Sync {
    fn dimension(&self) -> usize;
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub struct GeneratorFactory;

impl GeneratorFactory {
    pub fn create(config: ModelConfig) -> Result<Arc<dyn EmbeddingGenerator>> {
        Ok(Arc::new(FastEmbedGenerator::new(config)?))
    }
}
