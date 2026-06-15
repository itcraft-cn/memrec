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
        match config.model_type {
            memrec_common::ModelType::MiniLML6V2 => {
                Ok(Arc::new(FastEmbedGenerator::new(config)?))
            }
            memrec_common::ModelType::BGEM3 => {
                anyhow::bail!("BGE-M3 model is not yet supported. Please use MiniLML6V2.")
            }
            memrec_common::ModelType::Custom { .. } => {
                anyhow::bail!("Custom models are not yet supported. Please use MiniLML6V2.")
            }
        }
    }
}