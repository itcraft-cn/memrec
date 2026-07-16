//! # 搜索算法模块
//!
//! 提供搜索相关的算法组件：
//! - MMR 重排（多样性优化）
//! - 评分计算（时间衰减 + 源权重）

pub mod mmr;
pub mod scorer;

pub use mmr::{mmr_rerank, MmrConfig};
pub use scorer::{apply_scoring, ScorerConfig, SourceWeights};
