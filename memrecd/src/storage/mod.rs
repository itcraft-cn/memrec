//! # 存储层
//!
//! 提供 MemRec 的持久化存储能力，分为两个维度：
//!
//! - **记忆存储**（[`MemoryStore`]）：基于 RocksDB 的记忆 CRUD
//! - **向量存储**（[`VectorStore`] / [`RocksDBVectorStore`]）：嵌入向量的存储与语义搜索
//!
//! 底层 RocksDB 封装见 [`RocksDBStore`]，使用列族（Column Family）隔离不同数据。

pub mod hybrid_store;
pub mod memory_store;
pub mod rocksdb;
pub mod rocksdb_vector;
pub mod tantivy_store;
pub mod traits;
pub mod vector_store;

pub use hybrid_store::{HybridConfig, HybridStore};
pub use memory_store::MemoryStore;
pub use rocksdb::RocksDBStore;
pub use rocksdb_vector::RocksDBVectorStore;
pub use tantivy_store::TantivyStore;
pub use traits::{
    ConfigStorage, FtsPayload, FtsStorage, HybridSearchRequest, HybridSearchResult, HybridStorage,
    MemoryStorage, ProjectStorage, SearchFilter, SearchHit, VectorPayload, VectorStorage,
};
pub use vector_store::VectorStore;
