//! # 守护进程配置
//!
//! 从 `~/.memrec/config.toml` 加载配置，支持 `~` 路径展开。
//! 若配置文件不存在则自动生成默认配置并创建必要目录。
//!
//! ## 配置结构
//!
//! - [`DaemonConfig`]：顶层配置，包含模型配置和服务端配置
//! - [`DaemonServerConfig`]：服务端路径配置（Socket、数据目录、向量目录、日志文件）

use anyhow::Result;
use memrec_common::{ModelConfig, ModelType};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 守护进程顶层配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub model: ModelConfig,
    pub server: DaemonServerConfig,
}

/// 服务端路径配置。
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
    /// 从 `~/.memrec/config.toml` 加载配置。
    ///
    /// 若文件不存在，生成默认配置、创建数据目录并写入配置文件。
    /// 加载后自动展开路径中的 `~` 前缀。
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

    /// 配置文件路径：`~/.memrec/config.toml`
    fn config_path() -> PathBuf {
        let home = dirs::home_dir().expect("Failed to get home directory");
        home.join(".memrec/config.toml")
    }

    /// 展开所有路径中的 `~` 前缀为用户主目录。
    fn expand_paths(&mut self) {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        self.server.socket_path = expand_tilde(&self.server.socket_path, &home);
        self.server.data_dir = expand_tilde(&self.server.data_dir, &home);
        self.server.vectors_dir = expand_tilde(&self.server.vectors_dir, &home);
        self.server.log_file = expand_tilde(&self.server.log_file, &home);
    }

    /// 切换嵌入模型类型，返回新的配置。
    pub fn with_model(mut self, model_type: ModelType) -> Self {
        self.model = ModelConfig::new(model_type);
        self
    }

    /// 设置模型本地目录，返回新的配置。
    pub fn with_model_dir(mut self, model_dir: String) -> Self {
        self.model.model_dir = Some(model_dir);
        self
    }
}

/// 展开路径中的 `~` 前缀为用户主目录。
///
/// 若路径不以 `~` 开头则原样返回。
fn expand_tilde(path: &Path, home: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~") {
        home.join(rest)
    } else {
        path.to_path_buf()
    }
}
