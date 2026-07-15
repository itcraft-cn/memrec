use anyhow::Result;
use memrec_common::{ModelConfig, ModelType};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub model: ModelConfig,
    pub server: DaemonServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonServerConfig {
    pub socket_path: PathBuf,
    pub data_dir: PathBuf,
    pub vectors_dir: PathBuf,
    pub log_file: PathBuf,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let home = dirs::home_dir().expect("Failed to get home directory");

        let base_dir = home.join(".memrec");

        Self {
            model: ModelConfig::default(),
            server: DaemonServerConfig {
                socket_path: base_dir.join("memrecd.sock"),
                data_dir: base_dir.join("data"),
                vectors_dir: base_dir.join("vectors"),
                log_file: base_dir.join("memrecd.log"),
            },
        }
    }
}

impl DaemonConfig {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let mut config: DaemonConfig = toml::from_str(&content)?;
            config.expand_paths();
            Ok(config)
        } else {
            let config = Self::default();

            std::fs::create_dir_all(&config.server.data_dir)?;
            std::fs::create_dir_all(&config.server.vectors_dir)?;

            let toml = toml::to_string_pretty(&config)?;
            std::fs::write(config_path, toml)?;

            Ok(config)
        }
    }

    fn config_path() -> PathBuf {
        let home = dirs::home_dir().expect("Failed to get home directory");
        home.join(".memrec/config.toml")
    }

    fn expand_paths(&mut self) {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        self.server.socket_path = expand_tilde(&self.server.socket_path, &home);
        self.server.data_dir = expand_tilde(&self.server.data_dir, &home);
        self.server.vectors_dir = expand_tilde(&self.server.vectors_dir, &home);
        self.server.log_file = expand_tilde(&self.server.log_file, &home);
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

fn expand_tilde(path: &Path, home: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~") {
        home.join(rest)
    } else {
        path.to_path_buf()
    }
}
