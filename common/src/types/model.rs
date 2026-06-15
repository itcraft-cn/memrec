use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelType {
    #[serde(rename = "minilm-l6-v2")]
    MiniLML6V2,
    #[serde(rename = "bge-m3")]
    BGEM3,
    Custom { name: String, dimension: usize },
}

impl Default for ModelType {
    fn default() -> Self {
        Self::MiniLML6V2
    }
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
    
    pub fn huggingface_subpath(&self) -> Option<String> {
        match self {
            Self::MiniLML6V2 => None, // 默认根目录
            Self::BGEM3 => Some("onnx".to_string()), // 在onnx子目录
            Self::Custom { .. } => None,
        }
    }
    
    pub fn is_supported(&self) -> bool {
        match self {
            Self::MiniLML6V2 => true,
            Self::BGEM3 => true, // 现在支持BGE-M3
            Self::Custom { .. } => false, // 需要用户提供配置
        }
    }
    
    pub fn warning(&self) -> Option<String> {
        match self {
            Self::BGEM3 => Some("BGE-M3 is experimental and requires 2.3GB additional disk space".to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    pub filename: String,
    pub sha256: String,
    pub required: bool,
}

impl ModelFile {
    pub fn for_minilm() -> Vec<Self> {
        vec![
            ModelFile {
                filename: "model.onnx".to_string(),
                sha256: "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5".to_string(),
                required: true,
            },
            ModelFile {
                filename: "tokenizer.json".to_string(),
                sha256: "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0".to_string(),
                required: true,
            },
            ModelFile {
                filename: "config.json".to_string(),
                sha256: "1b4d8e2a3988377ed8b519a31d8d31025a25f1c5f8606998e8014111438efcd7".to_string(),
                required: true,
            },
            ModelFile {
                filename: "special_tokens_map.json".to_string(),
                sha256: "5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a".to_string(),
                required: true,
            },
            ModelFile {
                filename: "tokenizer_config.json".to_string(),
                sha256: "bd2e06a5b20fd1b13ca988bedc8763d332d242381b4fbc98f8fead4524158f79".to_string(),
                required: true,
            },
        ]
    }
    
    pub fn for_bge_m3() -> Vec<Self> {
        vec![
            ModelFile {
                filename: "model.onnx".to_string(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                required: true,
            },
            ModelFile {
                filename: "model.onnx_data".to_string(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                required: true,
            },
            ModelFile {
                filename: "sentencepiece.bpe.model".to_string(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                required: true,
            },
            ModelFile {
                filename: "tokenizer.json".to_string(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                required: true,
            },
            ModelFile {
                filename: "tokenizer_config.json".to_string(),
                sha256: "7e4c1cc848840aeccdd763458c18dd525eb0f795c992e00ebe9c28554e7db2d4".to_string(),
                required: true,
            },
            ModelFile {
                filename: "special_tokens_map.json".to_string(),
                sha256: "8c785abebea9ae3257b61681b4e6fd8365ceafde980c21970d001e834cf10835".to_string(),
                required: true,
            },
            ModelFile {
                filename: "config.json".to_string(),
                sha256: "f24afd5de914fba8c668426c43d208a1a54022500c63b2c160be20891686fce8".to_string(),
                required: true,
            },
            ModelFile {
                filename: "Constant_7_attr__value".to_string(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                required: true,
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_type: ModelType,
    pub source: String, // "huggingface", "local", "custom"
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
            ModelType::Custom { .. } => vec![], // 需要用户提供
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
        let repo = self.huggingface_repo()
            .unwrap_or_else(|| format!("custom-{}", self.model_type.name()));
        repo.replace('/', "--")
    }
}