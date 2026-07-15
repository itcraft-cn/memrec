use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelType {
    #[serde(rename = "minilm-l6-v2")]
    #[default]
    MiniLML6V2,
    #[serde(rename = "bge-m3")]
    BGEM3,
    Custom { name: String, dimension: usize },
}

impl ModelType {
    pub fn dimension(&self) -> usize {
        match self {
            Self::MiniLML6V2 => 384,
            Self::BGEM3 => 1024,
            Self::Custom { dimension, .. } => *dimension,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::MiniLML6V2 => "minilm-l6-v2".to_string(),
            Self::BGEM3 => "bge-m3".to_string(),
            Self::Custom { name, .. } => name.clone(),
        }
    }

    pub fn huggingface_repo(&self) -> Option<String> {
        match self {
            Self::MiniLML6V2 => Some("Qdrant/all-MiniLM-L6-v2-onnx".to_string()),
            Self::BGEM3 => Some("BAAI/bge-m3".to_string()),
            Self::Custom { .. } => None,
        }
    }

    pub fn is_supported(&self) -> bool {
        match self {
            Self::MiniLML6V2 => true,
            Self::BGEM3 => true,
            Self::Custom { .. } => false,
        }
    }

    pub fn warning(&self) -> Option<String> {
        match self {
            Self::BGEM3 => Some(
                "BGE-M3 requires ~2.3GB additional disk space for model files".to_string(),
            ),
            _ => None,
        }
    }

    pub fn pooling(&self) -> PoolingStrategy {
        match self {
            Self::MiniLML6V2 => PoolingStrategy::Mean,
            Self::BGEM3 => PoolingStrategy::Cls,
            Self::Custom { .. } => PoolingStrategy::Mean,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PoolingStrategy {
    Mean,
    Cls,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    pub filename: String,
    pub remote_path: String,
    pub sha256: String,
    pub file_type: ModelFileType,
    pub required: bool,
}

impl ModelFile {
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

    pub fn is_ready(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn huggingface_repo(&self) -> Option<String> {
        self.model_type.huggingface_repo()
    }

    pub fn local_dir_name(&self) -> String {
        let repo = self
            .huggingface_repo()
            .unwrap_or_else(|| format!("custom-{}", self.model_type.name()));
        repo.replace('/', "--")
    }

    pub fn onnx_model_file(&self) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.file_type == ModelFileType::OnnxModel)
    }

    pub fn external_data_files(&self) -> Vec<&ModelFile> {
        self.files
            .iter()
            .filter(|f| f.file_type == ModelFileType::OnnxExternalData)
            .collect()
    }

    pub fn tokenizer_file(&self) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.file_type == ModelFileType::Tokenizer)
    }

    pub fn config_file(&self) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.file_type == ModelFileType::Config)
    }

    pub fn special_tokens_map_file(&self) -> Option<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.file_type == ModelFileType::SpecialTokensMap)
    }

    pub fn tokenizer_config_file(&self) -> Option<&ModelFile> {
        self.files
            .iter()
            .find(|f| f.file_type == ModelFileType::TokenizerConfig)
    }
}
