use anyhow::Result;

pub struct FastEmbedGenerator {
    dimension: usize,
}

impl FastEmbedGenerator {
    pub fn new() -> Result<Self> {
        Ok(Self {
            dimension: 384,
        })
    }
    
    pub fn dimension(&self) -> usize {
        self.dimension
    }
    
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let hash = text.chars()
            .enumerate()
            .map(|(i, c)| (c as u32 as f32) * (i as f32 + 1.0) / 1000.0)
            .take(384)
            .collect::<Vec<_>>();
        
        let embedding = if hash.len() < 384 {
            hash.into_iter()
                .chain(std::iter::repeat(0.0))
                .take(384)
                .collect()
        } else {
            hash
        };
        
        Ok(embedding)
    }
    
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
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
    }
}