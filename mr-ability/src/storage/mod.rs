//! # 存储层
//!
//! 提供 MemRec 的持久化存储能力，分为三个维度：
//!
//! - **记忆存储**（[`MemoryStore`]）：基于 RocksDB 的记忆 CRUD
//! - **Facet 存储**（[`FacetStore`]）：基于 RocksDB 的 Facet CRUD + 向量检索
//! - **边存储**（[`EdgeStore`]）：基于 RocksDB 的图边 CRUD + 图遍历扩散
//! - **向量存储**（[`VectorStore`] / [`RocksDBVectorStore`]）：嵌入向量的存储与语义搜索
//! - **场景存储**（[`SceneStore`]）：基于 RocksDB 的场景 CRUD + 热度排序
//!
//! 底层 RocksDB 封装见 [`RocksDBStore`]，使用列族（Column Family）隔离不同数据。

pub mod dream_run_store;
pub mod edge_store;
pub mod facet_store;
pub mod hybrid_store;
pub mod indexing;
pub mod memory_store;
pub mod propagation_store;
pub mod rocksdb;
pub mod rocksdb_vector;
pub mod rule_store;
pub mod scene_store;
pub mod tantivy_store;
pub mod tiered_store;
pub mod traits;
pub mod vector_store;

pub use dream_run_store::DreamRunStore;
pub use edge_store::EdgeStore;
pub use facet_store::FacetStore;
pub use hybrid_store::{HybridConfig, HybridStore};
pub use indexing::IndexingStorage;
pub use memory_store::MemoryStore;
pub use propagation_store::PropagationStore;
pub use rocksdb::RocksDBStore;
pub use rocksdb_vector::RocksDBVectorStore;
pub use rule_store::RuleStore;
pub use scene_store::SceneStore;
pub use tantivy_store::TantivyStore;
pub use tiered_store::TieredStore;
pub use traits::{
    ConfigStorage, DreamRunStorage, EdgeStorage, FacetSearchHit, FacetStorage, FtsPayload,
    FtsStorage, GraphSearchHit, GraphTraversalParams, HybridSearchRequest, HybridSearchResult,
    HybridStorage, MemoryStorage, ProjectStorage, PropagationStorage, RuleStorage, SceneStorage,
    SearchFilter, SearchHit, TextProvider, TieredStorage, VectorPayload, VectorStorage,
};
pub use vector_store::VectorStore;
