use anyhow::Result;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use memrec_common::{ModelConfig, ModelType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub data_dir: PathBuf,
    pub vectors_dir: PathBuf,
    pub model: ModelConfig,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let home = dirs::home_dir()
            .expect("Failed to get home directory");
        
        let base_dir = home.join(".memrec");
        
        Self {
            socket_path: base_dir.join("memrecd.sock"),
            data_dir: base_dir.join("data"),
            vectors_dir: base_dir.join("vectors"),
            model: ModelConfig::default(),
        }
    }
}

impl DaemonConfig {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: DaemonConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = Self::default();
            
            // 确保目录存在
            std::fs::create_dir_all(&config.data_dir)?;
            std::fs::create_dir_all(&config.vectors_dir)?;
            
            // 保存默认配置
            let toml = toml::to_string_pretty(&config)?;
            std::fs::write(config_path, toml)?;
            
            Ok(config)
        }
    }
    
    fn config_path() -> PathBuf {
        let home = dirs::home_dir()
            .expect("Failed to get home directory");
        home.join(".memrec/config.toml")
    }
    
    pub fn with_model(mut self, model_type: ModelType) -> Self {
        self.model = ModelConfig::new(model_type);
        self
    }
    
    pub fn with_model_dir(mut self, model_dir: String) -> Self {
        self.model.model_dir = Some(model_dir);
        self
    }
}