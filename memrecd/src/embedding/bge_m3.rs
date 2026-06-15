use anyhow::Result;
use std::sync::Mutex;
use std::path::PathBuf;
use memrec_common::ModelConfig;

use super::EmbeddingGenerator;

const ENV_MODEL_DIR: &str = "MEMREC_MODEL_DIR";

/// 实验性的BGE-M3生成器
/// 注意：这可能需要额外的库或不同的tokenizer处理
pub struct BGEM3Generator {
    // 目前是空的 - 需要实现
    model_config: ModelConfig,
    initialized: bool,
}

impl BGEM3Generator {
    pub fn new(model_config: ModelConfig) -> Result<Self> {
        let model_dir = Self::get_model_dir(&model_config)?;
        
        // 检查模型文件
        for file in &model_config.files {
            let file_path = model_dir.join(&file.filename);
            if !file_path.exists() {
                anyhow::bail!("BGE-M3 model file missing: {} from {:?}. Download required.", 
                    file.filename, model_dir);
            }
        }
        
        tracing::warn!("BGE-M3 generator created but not fully implemented.");
        tracing::info!("Model directory: {:?}", model_dir);
        
        Ok(Self {
            model_config,
            initialized: false,
        })
    }
    
    fn get_model_dir(model_config: &ModelConfig) -> Result<PathBuf> {
        // 1. 环境变量优先
        if let Ok(env_path) = std::env::var(ENV_MODEL_DIR) {
            let path = PathBuf::from(env_path);
            if path.is_absolute() {
                return Ok(path);
            }
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
            return Ok(home.join(path));
        }
        
        // 2. 配置中的model_dir
        if let Some(ref model_dir) = model_config.model_dir {
            let path = PathBuf::from(model_dir);
            if path.is_absolute() {
                return Ok(path);
            }
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
            return Ok(home.join(path));
        }
        
        // 3. 默认路径
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
        let dir_name = model_config.local_dir_name();
        Ok(home.join(".memrec/models").join(dir_name))
    }
    
    fn ensure_initialized(&mut self) -> Result<()> {
        if !self.initialized {
            // TODO: 实现BGE-M3初始化
            // 这需要集成sentencepiece tokenizer和ONNX runtime
            anyhow::bail!("BGE-M3 implementation is not yet complete. Please use MiniLML6V2 for now.");
        }
        Ok(())
    }
}

impl EmbeddingGenerator for BGEM3Generator {
    fn dimension(&self) -> usize {
        1024 // BGE-M3维度是1024
    }
    
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        anyhow::bail!("BGE-M3 embedding not implemented. Use MiniLML6V2 instead.");
    }
    
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        anyhow::bail!("BGE-M3 batch embedding not implemented. Use MiniLML6V2 instead.");
    }
}