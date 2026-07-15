//! # 嵌入向量生成
//!
//! 定义 [`EmbeddingGenerator`] trait 和 [`GeneratorFactory`] 工厂。
//! 当前唯一实现为 [`FastEmbedGenerator`]（基于 ONNX Runtime 推理）。

pub mod fastembed;

pub use fastembed::FastEmbedGenerator;

use anyhow::Result;
use memrec_common::ModelConfig;
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
