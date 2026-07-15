use anyhow::Result;
use memrec_common::{ModelConfig as CommonModelConfig, ModelType};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallConfig {
    pub version: String,
    pub model: CommonModelConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub socket_path: String,
    pub data_dir: String,
    pub vectors_dir: String,
    pub log_file: String,
}

impl InstallConfig {
    pub fn new(model_type: ModelType) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: CommonModelConfig::new(model_type),
            server: ServerConfig {
                socket_path: "~/.memrec/memrecd.sock".to_string(),
                data_dir: "~/.memrec/data".to_string(),
                vectors_dir: "~/.memrec/vectors".to_string(),
                log_file: "~/.memrec/memrecd.log".to_string(),
            },
        }
    }
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self::new(ModelType::default())
    }
}

pub fn generate_config(home: &Path, model_type: &ModelType) -> Result<()> {
    let config = InstallConfig::new(model_type.clone());
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
        assert_eq!(config.model.dimension, 384);
        assert_eq!(config.model.source, "huggingface");
        assert_eq!(config.model.model_type.name(), "minilm-l6-v2");
    }

    #[test]
    fn test_config_bge_m3() {
        let config = InstallConfig::new(ModelType::BGEM3);
        assert_eq!(config.model.dimension, 1024);
        assert_eq!(config.model.model_type.name(), "bge-m3");
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let config = InstallConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: InstallConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.version, parsed.version);
        assert_eq!(config.model.dimension, parsed.model.dimension);
        assert_eq!(config.server.socket_path, parsed.server.socket_path);
    }

    #[test]
    fn test_generate_config() {
        let dir = tempdir().unwrap();
        generate_config(dir.path(), &ModelType::MiniLML6V2).unwrap();

        let config_path = dir.path().join("config.toml");
        assert!(config_path.exists());

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("dimension = 384"));
        assert!(content.contains("model_type"));

        let parsed: InstallConfig = toml::from_str(&content).unwrap();
        assert_eq!(parsed.model.dimension, 384);
    }

    #[test]
    fn test_generate_config_bge_m3() {
        let dir = tempdir().unwrap();
        generate_config(dir.path(), &ModelType::BGEM3).unwrap();

        let config_path = dir.path().join("config.toml");
        assert!(config_path.exists());

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("dimension = 1024"));
        assert!(content.contains("bge-m3"));
    }
}
