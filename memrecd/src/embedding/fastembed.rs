use anyhow::Result;
use fastembed::{Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel};
use memrec_common::{ModelConfig, ModelFileType, PoolingStrategy};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::EmbeddingGenerator;

const ENV_MODEL_DIR: &str = "MEMREC_MODEL_DIR";

pub struct FastEmbedGenerator {
    model: Mutex<TextEmbedding>,
    model_config: ModelConfig,
}

impl FastEmbedGenerator {
    pub fn new(model_config: ModelConfig) -> Result<Self> {
        let model_dir = Self::get_model_dir(&model_config)?;

        for file in &model_config.files {
            if file.required {
                let file_path = model_dir.join(&file.filename);
                if !file_path.exists() {
                    anyhow::bail!(
                        "Model file missing: {} from {:?}. Download required.",
                        file.filename,
                        model_dir
                    );
                }
            }
        }

        let onnx_file = model_config
            .onnx_model_file()
            .ok_or_else(|| anyhow::anyhow!("No ONNX model file in config"))?;
        let onnx_data = std::fs::read(model_dir.join(&onnx_file.filename))
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", onnx_file.filename, e))?;

        let tokenizer_files = TokenizerFiles {
            tokenizer_file: read_model_file(&model_dir, &model_config, ModelFileType::Tokenizer)?,
            config_file: read_model_file(&model_dir, &model_config, ModelFileType::Config)?,
            special_tokens_map_file: read_model_file(
                &model_dir,
                &model_config,
                ModelFileType::SpecialTokensMap,
            )?,
            tokenizer_config_file: read_model_file(
                &model_dir,
                &model_config,
                ModelFileType::TokenizerConfig,
            )?,
        };

        let mut user_model = UserDefinedEmbeddingModel::new(onnx_data, tokenizer_files);

        for ext_file in model_config.external_data_files() {
            let data = std::fs::read(model_dir.join(&ext_file.filename)).map_err(|e| {
                anyhow::anyhow!("Failed to read {}: {}", ext_file.filename, e)
            })?;
            user_model = user_model.with_external_initializer(ext_file.filename.clone(), data);
        }

        user_model = match model_config.model_type.pooling() {
            PoolingStrategy::Cls => user_model.with_pooling(Pooling::Cls),
            PoolingStrategy::Mean => user_model.with_pooling(Pooling::Mean),
        };

        let model = TextEmbedding::try_new_from_user_defined(user_model, Default::default())?;

        Ok(Self {
            model: Mutex::new(model),
            model_config,
        })
    }

    fn get_model_dir(model_config: &ModelConfig) -> Result<PathBuf> {
        if let Ok(env_path) = std::env::var(ENV_MODEL_DIR) {
            let path = PathBuf::from(env_path);
            if path.is_absolute() {
                return Ok(path);
            }
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
            return Ok(home.join(path));
        }

        if let Some(ref model_dir) = model_config.model_dir {
            let path = PathBuf::from(model_dir);
            if path.is_absolute() {
                return Ok(path);
            }
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
            return Ok(home.join(path));
        }

        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
        let dir_name = model_config.local_dir_name();
        Ok(home.join(".memrec/models").join(dir_name))
    }
}

fn read_model_file(
    model_dir: &Path,
    model_config: &ModelConfig,
    file_type: ModelFileType,
) -> Result<Vec<u8>> {
    let mf = model_config
        .files
        .iter()
        .find(|f| f.file_type == file_type)
        .ok_or_else(|| anyhow::anyhow!("No {:?} file in model config", file_type))?;
    std::fs::read(model_dir.join(&mf.filename))
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", mf.filename, e))
}

impl EmbeddingGenerator for FastEmbedGenerator {
    fn dimension(&self) -> usize {
        self.model_config.dimension
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("Model lock poisoned"))?;
        let embeddings = model.embed(vec![text], None)?;

        embeddings
            .into_iter()
            .next()
            .map(|e| e.into_iter().collect())
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("Model lock poisoned"))?;
        let embeddings = model.embed(texts, None)?;

        Ok(embeddings
            .into_iter()
            .map(|e| e.into_iter().collect())
            .collect())
    }
}

impl Default for FastEmbedGenerator {
    fn default() -> Self {
        let model_config = ModelConfig::default();
        Self::new(model_config).expect("Failed to initialize FastEmbed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_dimension() {
        let model_config = ModelConfig::default();
        let generator = FastEmbedGenerator::new(model_config).unwrap();
        assert_eq!(generator.dimension(), 384);
    }

    #[test]
    fn test_single_embedding() {
        let model_config = ModelConfig::default();
        let generator = FastEmbedGenerator::new(model_config).unwrap();
        let embedding = generator.embed("test text").unwrap();

        assert_eq!(embedding.len(), 384);

        let non_zero_count = embedding.iter().filter(|v| **v != 0.0).count();
        assert!(non_zero_count > 100);
    }

    #[test]
    fn test_semantic_similarity() {
        let model_config = ModelConfig::default();
        let generator = FastEmbedGenerator::new(model_config).unwrap();

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

        assert!(
            sim12 > sim13,
            "语义相似度测试失败: 狗猫应比狗汽车更相似"
        );
    }
}
