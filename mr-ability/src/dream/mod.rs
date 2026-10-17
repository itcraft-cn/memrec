//! # Dream 后台整合模块
//!
//! 实现记忆压缩和整合，类似人类睡眠中的记忆巩固过程。
//!
//! ## 架构
//!
//! - [`DreamProcessor`]：核心处理器，执行记忆整合
//! - [`DreamLock`]：并发控制，防止多个 Dream 同时执行
//! - [`DreamGate`]：门控检查，决定是否触发 Dream
//!
//! ## 流程
//!
//! 1. 门控检查（enabled / time / sessions）
//! 2. 获取 DreamLock
//! 3. 筛选符合条件的旧记忆
//! 4. 调用 LLM 整合生成摘要
//! 5. 写入新记忆，删除旧记忆
//! 6. 释放锁

mod fusion;
mod gate;
mod lock;
mod networking;
mod phases;
mod processor;

pub use fusion::{
    cluster_candidates, cosine, extract_cluster, ClusterExtraction, ContradictionItem, FactItem,
};
pub use gate::{DreamGate, DreamGateResult};
pub use lock::{DreamLock, DreamState, LockContent};
pub use networking::EdgeBuilder;
pub use phases::{
    CrossProjectExtractor, MemoryCleaner, PersonalSummarizer, PhaseResult, VectorRegenerator,
};
pub use processor::{extract_summary_clues, DreamProcessor, DreamResult};
