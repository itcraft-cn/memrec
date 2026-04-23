use anyhow::Result;
use fastembed::{TextEmbedding, UserDefinedEmbeddingModel, TokenizerFiles};
use std::sync::Mutex;

const MODEL_DIR: &str = ".cache/huggingface/hub/models--Qdrant--all-MiniLM-L6-v2-onnx/snapshots/main";

pub struct FastEmbedGenerator {
    model: Mutex<TextEmbedding>,
    dimension: usize,
}

impl FastEmbedGenerator {
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
        
        let model_dir = home.join(MODEL_DIR);
        
        let onnx_file = std::fs::read(model_dir.join("model.onnx"))
            .map_err(|e| anyhow::anyhow!("Failed to read model.onnx: {}. Download from https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx", e))?;
        
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: std::fs::read(model_dir.join("tokenizer.json"))?,
            config_file: std::fs::read(model_dir.join("config.json"))?,
            special_tokens_map_file: std::fs::read(model_dir.join("special_tokens_map.json"))?,
            tokenizer_config_file: std::fs::read(model_dir.join("tokenizer_config.json"))?,
        };
        
        let user_model = UserDefinedEmbeddingModel::new(onnx_file, tokenizer_files);
        
        let model = TextEmbedding::try_new_from_user_defined(user_model, Default::default())?;
        
        Ok(Self {
            model: Mutex::new(model),
            dimension: 384,
        })
    }
    
    pub fn dimension(&self) -> usize {
        self.dimension
    }
    
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut model = self.model.lock().map_err(|_| anyhow::anyhow!("Model lock poisoned"))?;
        let embeddings = model.embed(vec![text], None)?;
        
        embeddings.into_iter()
            .next()
            .map(|e| e.into_iter().map(|v| v as f32).collect())
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }
    
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut model = self.model.lock().map_err(|_| anyhow::anyhow!("Model lock poisoned"))?;
        let embeddings = model.embed(texts.to_vec(), None)?;
        
        Ok(embeddings.into_iter()
            .map(|e| e.into_iter().map(|v| v as f32).collect())
            .collect())
    }
}

impl Default for FastEmbedGenerator {
    fn default() -> Self {
        Self::new().expect("Failed to initialize FastEmbed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_embedding_dimension() {
        let generator = FastEmbedGenerator::new().unwrap();
        assert_eq!(generator.dimension(), 384);
    }
    
    #[test]
    fn test_single_embedding() {
        let generator = FastEmbedGenerator::new().unwrap();
        let embedding = generator.embed("test text").unwrap();
        
        assert_eq!(embedding.len(), 384);
        
        let non_zero_count = embedding.iter().filter(|v| **v != 0.0).count();
        assert!(non_zero_count > 100);
    }
    
    #[test]
    fn test_semantic_similarity() {
        let generator = FastEmbedGenerator::new().unwrap();
        
        let emb1 = generator.embed("狗是动物").unwrap();
        let emb2 = generator.embed("猫是动物").unwrap();
        let emb3 = generator.embed("汽车是机器").unwrap();
        
        fn cosine(a: &[f32], b: &[f32]) -> f32 {
            let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
            let norm_a = (a.iter().map(|x| x * x).sum::<f32>()).sqrt();
            let norm_b = (b.iter().map(|x| x * x).sum::<f32>()).sqrt();
            dot / (norm_a * norm_b)
        }
        
        let sim12 = cosine(&emb1, &emb2);
        let sim13 = cosine(&emb1, &emb3);
        
        println!("狗-猫相似度: {}", sim12);
        println!("狗-汽车相似度: {}", sim13);
        
        assert!(sim12 > sim13, "语义相似度测试失败: 狗猫应比狗汽车更相似");
    }
}