//! # 嵌入模型抽象
//!
//! 定义语义搜索所需的嵌入模型类型、文件清单和配置。
//!
//! ## 模型类型
//!
//! [`ModelType`] 支持三种模型：
//!
//! - **MiniLM-L6-v2**（默认）：384 维，轻量级，适合本地部署
//! - **BGE-M3**：1024 维，高精度，需约 2.3GB 额外磁盘空间
//! - **Custom**：用户自定义模型，需指定名称和维度
//!
//! ## 模型文件
//!
//! [`ModelFile`] 描述模型所需的每个文件（ONNX 模型、分词器、配置等），
//! 包含远程路径和 SHA256 校验和，用于下载验证。
//! [`ModelFile::for_minilm`] 和 [`ModelFile::for_bge_m3`] 分别返回两种预置模型的文件清单。
//!
//! ## 模型配置
//!
//! [`ModelConfig`] 聚合模型类型、文件清单和本地路径，
//! 提供按类型查找文件的便捷方法（如 [`ModelConfig::onnx_model_file`]）。

use serde::{Deserialize, Serialize};

/// 嵌入模型类型枚举。
///
/// 序列化为 kebab-case（如 `"minilm-l6-v2"`、`"bge-m3"`），
/// `Custom` 变体序列化为 `{"custom": {"name": "...", "dimension": ...}}`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelType {
    #[serde(rename = "minilm-l6-v2")]
    #[default]
    MiniLML6V2,
    #[serde(rename = "bge-m3")]
    BGEM3,
    Custom {
        name: String,
        dimension: usize,
    },
}

impl ModelType {
    /// 返回该模型的嵌入向量维度。
    pub fn dimension(&self) -> usize {
        match self {
            Self::MiniLML6V2 => 384,
            Self::BGEM3 => 1024,
            Self::Custom { dimension, .. } => *dimension,
        }
    }

    /// 返回模型的标准名称字符串。
    pub fn name(&self) -> String {
        match self {
            Self::MiniLML6V2 => "minilm-l6-v2".to_string(),
            Self::BGEM3 => "bge-m3".to_string(),
            Self::Custom { name, .. } => name.clone(),
        }
    }

    /// 返回 HuggingFace 仓库路径，自定义模型返回 `None`。
    pub fn huggingface_repo(&self) -> Option<String> {
        match self {
            Self::MiniLML6V2 => Some("Qdrant/all-MiniLM-L6-v2-onnx".to_string()),
            Self::BGEM3 => Some("BAAI/bge-m3".to_string()),
            Self::Custom { .. } => None,
        }
    }

    /// 判断该模型是否为内置支持模型。
    ///
    /// 内置模型（MiniLM-L6-v2、BGE-M3）可自动下载和验证，
    /// 自定义模型需用户自行提供文件。
    pub fn is_supported(&self) -> bool {
        match self {
            Self::MiniLML6V2 => true,
            Self::BGEM3 => true,
            Self::Custom { .. } => false,
        }
    }

    /// 返回模型使用注意事项，当前仅 BGE-M3 有磁盘空间警告。
    pub fn warning(&self) -> Option<String> {
        match self {
            Self::BGEM3 => {
                Some("BGE-M3 requires ~2.3GB additional disk space for model files".to_string())
            }
            _ => None,
        }
    }

    /// 返回该模型推荐的语义搜索最低相似度阈值。
    ///
    /// MiniLM-L6-v2 的嵌入空间较紧凑，推荐 0.75；
    /// BGE-M3 的嵌入空间较稀疏，推荐 0.5。
    pub fn default_min_score(&self) -> f32 {
        match self {
            Self::MiniLML6V2 => 0.75,
            Self::BGEM3 => 0.5,
            Self::Custom { .. } => 0.5,
        }
    }

    /// 返回该模型的池化策略。
    ///
    /// MiniLM-L6-v2 使用均值池化（Mean），BGE-M3 使用 CLS token 池化。
    pub fn pooling(&self) -> PoolingStrategy {
        match self {
            Self::MiniLML6V2 => PoolingStrategy::Mean,
            Self::BGEM3 => PoolingStrategy::Cls,
            Self::Custom { .. } => PoolingStrategy::Mean,
        }
    }
}

/// 嵌入向量的池化策略。
///
/// - `Mean`：对所有 token 嵌入取均值
/// - `Cls`：取 `[CLS]` token 的嵌入作为句子表示
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PoolingStrategy {
    Mean,
    Cls,
}

/// 模型文件类型枚举。
///
/// 用于标识模型文件清单中每个文件的用途，
/// 便于按类型查找（如查找 ONNX 模型文件、分词器文件等）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelFileType {
    OnnxModel,
    OnnxExternalData,
    Tokenizer,
    Config,
    SpecialTokensMap,
    TokenizerConfig,
    SentencePieceModel,
}

/// 模型文件描述。
///
/// 记录每个模型文件的本地文件名、远程下载路径、SHA256 校验和、
/// 文件类型和是否必需。下载时通过校验和验证文件完整性。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    pub filename: String,
    pub remote_path: String,
    pub sha256: String,
    pub file_type: ModelFileType,
    pub required: bool,
}

impl ModelFile {
    /// 返回 MiniLM-L6-v2 模型的完整文件清单。
    ///
    /// 包含 ONNX 模型、分词器、配置文件和特殊 token 映射，
    /// 所有文件均来自 `Qdrant/all-MiniLM-L6-v2-onnx` 仓库。
    pub fn for_minilm() -> Vec<Self> {
        vec![
            ModelFile {
                filename: "model.onnx".to_string(),
                remote_path: "model.onnx".to_string(),
                sha256: "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
                    .to_string(),
                file_type: ModelFileType::OnnxModel,
                required: true,
            },
            ModelFile {
                filename: "tokenizer.json".to_string(),
                remote_path: "tokenizer.json".to_string(),
                sha256: "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"
                    .to_string(),
                file_type: ModelFileType::Tokenizer,
                required: true,
            },
            ModelFile {
                filename: "config.json".to_string(),
                remote_path: "config.json".to_string(),
                sha256: "1b4d8e2a3988377ed8b519a31d8d31025a25f1c5f8606998e8014111438efcd7"
                    .to_string(),
                file_type: ModelFileType::Config,
                required: true,
            },
            ModelFile {
                filename: "special_tokens_map.json".to_string(),
                remote_path: "special_tokens_map.json".to_string(),
                sha256: "5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a"
                    .to_string(),
                file_type: ModelFileType::SpecialTokensMap,
                required: true,
            },
            ModelFile {
                filename: "tokenizer_config.json".to_string(),
                remote_path: "tokenizer_config.json".to_string(),
                sha256: "bd2e06a5b20fd1b13ca988bedc8763d332d242381b4fbc98f8fead4524158f79"
                    .to_string(),
                file_type: ModelFileType::TokenizerConfig,
                required: true,
            },
        ]
    }

    /// 返回 BGE-M3 模型的完整文件清单。
    ///
    /// 包含 ONNX 模型、外部数据文件、分词器、配置文件、特殊 token 映射
    /// 和 SentencePiece 模型。文件来自 `BAAI/bge-m3` 仓库的 `onnx/` 子目录。
    /// `sentencepiece.bpe.model` 为可选文件（`required: false`）。
    pub fn for_bge_m3() -> Vec<Self> {
        vec![
            ModelFile {
                filename: "model.onnx".to_string(),
                remote_path: "onnx/model.onnx".to_string(),
                sha256: "f84251230831afb359ab26d9fd37d5936d4d9bb5d1d5410e66442f630f24435b"
                    .to_string(),
                file_type: ModelFileType::OnnxModel,
                required: true,
            },
            ModelFile {
                filename: "model.onnx_data".to_string(),
                remote_path: "onnx/model.onnx_data".to_string(),
                sha256: "1eebfb28493f67bba03ce0ef64bfdc7fc5a3bd9d7493f818bb1d78cd798416b4"
                    .to_string(),
                file_type: ModelFileType::OnnxExternalData,
                required: true,
            },
            ModelFile {
                filename: "Constant_7_attr__value".to_string(),
                remote_path: "onnx/Constant_7_attr__value".to_string(),
                sha256: "cdf16f72c5d07b36484056e601ed9687f78477e5d85cee85a34f2406b7fb5906"
                    .to_string(),
                file_type: ModelFileType::OnnxExternalData,
                required: true,
            },
            ModelFile {
                filename: "tokenizer.json".to_string(),
                remote_path: "tokenizer.json".to_string(),
                sha256: "6710678b12670bc442b99edc952c4d996ae309a7020c1fa0096dd245c2faf790"
                    .to_string(),
                file_type: ModelFileType::Tokenizer,
                required: true,
            },
            ModelFile {
                filename: "config.json".to_string(),
                remote_path: "config.json".to_string(),
                sha256: "f24afd5de914fba8c668426c43d208a1a54022500c63b2c160be20891686fce8"
                    .to_string(),
                file_type: ModelFileType::Config,
                required: true,
            },
            ModelFile {
                filename: "special_tokens_map.json".to_string(),
                remote_path: "special_tokens_map.json".to_string(),
                sha256: "8c785abebea9ae3257b61681b4e6fd8365ceafde980c21970d001e834cf10835"
                    .to_string(),
                file_type: ModelFileType::SpecialTokensMap,
                required: true,
            },
            ModelFile {
                filename: "tokenizer_config.json".to_string(),
                remote_path: "tokenizer_config.json".to_string(),
                sha256: "7e4c1cc848840aeccdd763458c18dd525eb0f795c992e00ebe9c28554e7db2d4"
                    .to_string(),
                file_type: ModelFileType::TokenizerConfig,
                required: true,
            },
            ModelFile {
                filename: "sentencepiece.bpe.model".to_string(),
                remote_path: "sentencepiece.bpe.model".to_string(),
                sha256: "cfc8146abe2a0488e9e2a0c56de7952f7c11ab059eca145a0a727afce0db2865"
                    .to_string(),
                file_type: ModelFileType::SentencePieceModel,
                required: false,
            },
        ]
    }
}

/// 模型配置，聚合模型类型、文件清单和本地路径。
///
/// 由 [`memrecd::config::DaemonConfig`] 加载，也可通过 [`ModelConfig::new`] 按模型类型构造。
/// `model_dir` 在模型下载完成后由守护进程设置，指向本地缓存目录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_type: ModelType,
    pub source: String,
    pub dimension: usize,
    pub files: Vec<ModelFile>,
    pub model_dir: Option<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_type: ModelType::default(),
            source: "huggingface".to_string(),
            dimension: ModelType::default().dimension(),
            files: ModelFile::for_minilm(),
            model_dir: None,
        }
    }
}

impl ModelConfig {
    /// 按指定模型类型构造配置，自动填充维度和文件清单。
    pub fn new(model_type: ModelType) -> Self {
        let dimension = model_type.dimension();
        let files = match &model_type {
            ModelType::MiniLML6V2 => ModelFile::for_minilm(),
            ModelType::BGEM3 => ModelFile::for_bge_m3(),
            ModelType::Custom { .. } => vec![],
        };

        Self {
            model_type,
            source: "huggingface".to_string(),
            dimension,
            files,
            model_dir: None,
        }
    }

    /// 判断模型是否就绪（文件清单非空）。
    pub fn is_ready(&self) -> bool {
        !self.files.is_empty()
    }

    /// 返回 HuggingFace 仓库路径，委托给 [`ModelType::huggingface_repo`]。
    pub fn huggingface_repo(&self) -> Option<String> {
        self.model_type.huggingface_repo()
    }

    /// 生成本地缓存目录名。
    ///
    /// 将 HuggingFace 仓库路径中的 `/` 替换为 `--`，
    /// 如 `Qdrant/all-MiniLM-L6-v2-onnx` → `Qdrant--all-MiniLM-L6-v2-onnx`。
    /// 自定义模型使用 `custom-{name}` 格式。
    pub fn local_dir_name(&self) -> String {
        let repo = self
            .huggingface_repo()
            .unwrap_or_else(|| format!("custom-{}", self.model_type.name()));
        repo.replace('/', "--")
    }

    /// 查找 ONNX 模型文件。
    pub fn onnx_model_file(&self) -> Option<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.file_type == ModelFileType::OnnxModel)
    }

    /// 查找所有 ONNX 外部数据文件（如 `model.onnx_data`）。
    pub fn external_data_files(&self) -> Vec<&ModelFile> {
        self.files
            .iter()
            .filter(|f| f.file_type == ModelFileType::OnnxExternalData)
            .collect()
    }

    /// 查找分词器文件（`tokenizer.json`）。
    pub fn tokenizer_file(&self) -> Option<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.file_type == ModelFileType::Tokenizer)
    }

    /// 查找模型配置文件（`config.json`）。
    pub fn config_file(&self) -> Option<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.file_type == ModelFileType::Config)
    }

    /// 查找特殊 token 映射文件（`special_tokens_map.json`）。
    pub fn special_tokens_map_file(&self) -> Option<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.file_type == ModelFileType::SpecialTokensMap)
    }

    /// 查找分词器配置文件（`tokenizer_config.json`）。
    pub fn tokenizer_config_file(&self) -> Option<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.file_type == ModelFileType::TokenizerConfig)
    }
}
