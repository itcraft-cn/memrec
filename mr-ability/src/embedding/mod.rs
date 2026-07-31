//! # 嵌入向量生成
//!
//! 定义 [`EmbeddingGenerator`] trait 和 [`GeneratorFactory`] 工厂。
//! 当前唯一实现为 [`FastEmbedGenerator`]（基于 ONNX Runtime 推理）。
//!
//! [`MockEmbedder`] 用于测试，无需加载模型文件。

pub mod fastembed;

pub use fastembed::FastEmbedGenerator;

use anyhow::Result;
use mr_common::ModelConfig;
use std::sync::Arc;

/// 嵌入向量生成器 trait。
///
/// 将文本转换为固定维度的浮点向量，用于语义搜索。
/// 实现必须线程安全（`Send + Sync`）。
pub trait EmbeddingGenerator: Send + Sync {
    /// 返回嵌入向量维度。
    fn dimension(&self) -> usize;
    /// 生成单条文本的嵌入向量。
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// 批量生成嵌入向量。
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// 嵌入生成器工厂，根据模型配置创建对应实现。
pub struct GeneratorFactory;

impl GeneratorFactory {
    /// 创建嵌入生成器实例。
    ///
    /// 当前始终返回 [`FastEmbedGenerator`]，后续可扩展其他后端。
    pub fn create(config: ModelConfig) -> Result<Arc<dyn EmbeddingGenerator>> {
        Ok(Arc::new(FastEmbedGenerator::new(config)?))
    }
}

/// Mock 嵌入生成器，用于测试。
///
/// 无需加载模型文件，返回固定维度（384）的零向量。
pub struct MockEmbedder;

impl MockEmbedder {
    /// 创建新的 Mock 嵌入生成器。
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingGenerator for MockEmbedder {
    fn dimension(&self) -> usize {
        384
    }

    fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; 384])
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0; 384]).collect())
    }
}
