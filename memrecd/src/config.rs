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
use mr_common::{
    DedupConfig, DreamConfig, IdleMonitorConfig, LlmConfig, LogConfig, ModelConfig, ModelType,
    QueueConfig,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 守护进程顶层配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub model: ModelConfig,
    pub server: DaemonServerConfig,
    #[serde(default)]
    pub dream: DreamConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub dedup: DedupConfig,
    #[serde(default)]
    pub archive: ArchiveConfig,
    #[serde(default)]
    pub idle_monitor: IdleMonitorConfig,
    #[serde(default)]
    pub queue: QueueConfig,
    #[serde(default)]
    pub log: LogConfig,
}

/// 服务端路径配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonServerConfig {
    pub socket_path: PathBuf,
    pub data_dir: PathBuf,
    pub vectors_dir: PathBuf,
}

/// 存档配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveConfig {
    #[serde(default = "default_archive_enabled")]
    pub enabled: bool,
}

fn default_archive_enabled() -> bool {
    true
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            enabled: default_archive_enabled(),
        }
    }
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
            },
            dream: DreamConfig::default(),
            llm: LlmConfig::default(),
            dedup: DedupConfig::default(),
            archive: ArchiveConfig::default(),
            idle_monitor: IdleMonitorConfig::default(),
            queue: QueueConfig::default(),
            log: LogConfig::default(),
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
            // 展开 [llm] api_key 中的环境变量引用（如 $MEMREC_LLM_KEY）
            config.llm.resolve_env_api_key();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_config_defaults_include_llm_and_dedup() {
        let config = DaemonConfig::default();
        // LLM 默认关闭，Dream 需 LLM 时视为不启用
        assert!(!config.llm.enabled);
        assert_eq!(config.llm.provider, "deepseek");
        assert_eq!(config.llm.think_level(), mr_common::ThinkLevel::Medium);
        assert!(config.dedup.enabled);
        assert_eq!(config.dedup.top_k, 5);
        assert_eq!(config.dedup.duplicate_threshold, 0.92);
    }

    #[test]
    fn test_daemon_config_parses_llm_section() {
        let toml_str = r#"
            [server]
            socket_path = "~/.memrec/memrecd.sock"
            data_dir = "~/.memrec/data"
            vectors_dir = "~/.memrec/vectors"

            [model]
            model_type = "bge-m3"
            source = "huggingface"
            dimension = 1024
            files = []

            [llm]
            enabled = true
            provider = "deepseek"
            url = "https://api.deepseek.com/v1"
            api_key = "sk-test"
            model = "deepseek-v4-flash"
            think_level = "low"

            [dedup]
            enabled = true
            top_k = 10
            duplicate_threshold = 0.95
            update_threshold = 0.88

            [dream]
            enabled = true
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert!(config.llm.enabled);
        assert_eq!(config.llm.api_key, "sk-test");
        assert_eq!(config.llm.model, "deepseek-v4-flash");
        assert_eq!(config.llm.think_level(), mr_common::ThinkLevel::Low);
        assert_eq!(config.dedup.top_k, 10);
        assert_eq!(config.dedup.duplicate_threshold, 0.95);
        assert_eq!(config.dedup.update_threshold, 0.88);
        assert!(config.dream.enabled);
        assert!(config.dream.requires_llm);
    }

    #[test]
    fn test_daemon_config_llm_off_by_default() {
        let toml_str = r#"
            [server]
            socket_path = "~/.memrec/memrecd.sock"
            data_dir = "~/.memrec/data"
            vectors_dir = "~/.memrec/vectors"

            [model]
            model_type = "minilm-l6-v2"
            source = "huggingface"
            dimension = 384
            files = []
        "#;
        let config: DaemonConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.llm.enabled);
        assert!(config.dedup.enabled);
        assert_eq!(config.llm.max_tokens, 4096);
    }

    #[test]
    fn test_daemon_config_resolves_llm_api_key_env() {
        std::env::set_var("MEMREC_TEST_CFG_LLM_KEY", "sk-from-env");
        let toml_str = r#"
            [server]
            socket_path = "~/.memrec/memrecd.sock"
            data_dir = "~/.memrec/data"
            vectors_dir = "~/.memrec/vectors"

            [model]
            model_type = "bge-m3"
            source = "huggingface"
            dimension = 1024
            files = []

            [llm]
            enabled = true
            api_key = "$MEMREC_TEST_CFG_LLM_KEY"
        "#;
        let mut config: DaemonConfig = toml::from_str(toml_str).unwrap();
        config.llm.resolve_env_api_key();
        assert_eq!(config.llm.api_key, "sk-from-env");
        std::env::remove_var("MEMREC_TEST_CFG_LLM_KEY");
    }
}
