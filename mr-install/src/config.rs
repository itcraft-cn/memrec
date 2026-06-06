use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallConfig {
    pub version: String,
    pub model: ModelConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub source: String,
    pub dimension: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub socket_path: String,
    pub data_dir: String,
    pub vectors_dir: String,
    pub log_file: String,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: ModelConfig {
                name: "Qdrant/all-MiniLM-L6-v2-onnx".to_string(),
                source: "huggingface".to_string(),
                dimension: 384,
            },
            server: ServerConfig {
                socket_path: "~/.memrec/memrecd.sock".to_string(),
                data_dir: "~/.memrec/data".to_string(),
                vectors_dir: "~/.memrec/vectors".to_string(),
                log_file: "~/.memrec/memrecd.log".to_string(),
            },
        }
    }
}

pub fn generate_config(home: &Path) -> Result<()> {
    let config = InstallConfig::default();
    let config_path = home.join("config.toml");
    
    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, toml_str)?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_config_default() {
        let config = InstallConfig::default();
        assert_eq!(config.model.name, "Qdrant/all-MiniLM-L6-v2-onnx");
        assert_eq!(config.model.dimension, 384);
        assert_eq!(config.model.source, "huggingface");
    }
    
    #[test]
    fn test_config_toml_roundtrip() {
        let config = InstallConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: InstallConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.version, parsed.version);
        assert_eq!(config.model.name, parsed.model.name);
        assert_eq!(config.model.dimension, parsed.model.dimension);
        assert_eq!(config.server.socket_path, parsed.server.socket_path);
    }
    
    #[test]
    fn test_generate_config() {
        let dir = tempdir().unwrap();
        generate_config(dir.path()).unwrap();
        
        let config_path = dir.path().join("config.toml");
        assert!(config_path.exists());
        
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("Qdrant/all-MiniLM-L6-v2-onnx"));
        assert!(content.contains("dimension = 384"));
        
        let parsed: InstallConfig = toml::from_str(&content).unwrap();
        assert_eq!(parsed.model.dimension, 384);
    }
}
