//! 混合搜索集成测试的共享前置（导入与通用依赖）。
//!
//! 作为 `tests/common/mod.rs`，不会被当作独立测试 crate 编译。
//! 各测试文件通过 `mod common; use common::*;` 引入。
#![allow(unused_imports)]

pub use mr_ability::search::{apply_scoring, MmrConfig, ScorerConfig, SourceWeights};
pub use mr_ability::storage::{
    FtsPayload, FtsStorage, HybridSearchRequest, HybridStorage, HybridStore, RocksDBVectorStore,
    SearchFilter, TantivyStore, VectorPayload, VectorStorage, VectorStore,
};
pub use mr_common::MemoryScope;
pub use std::sync::Arc;
pub use tempfile::TempDir;
pub use uuid::Uuid;
