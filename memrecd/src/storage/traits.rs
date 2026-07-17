//! # 存储层 trait 定义
//!
//! 定义记忆存储、项目存储、配置存储、Facet 存储和向量存储的抽象接口，
//! 便于替换底层实现（如未来支持其他数据库后端）。

use anyhow::Result;
use async_trait::async_trait;
use memrec_common::{Memory, MemoryEdge, MemoryFacet, MemoryType, PropagationDelta};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 记忆存储 trait。
///
/// 提供记忆的 CRUD、按类型/标签/重要性/项目查询、软删除管理等功能。
#[async_trait]
pub trait MemoryStorage: Send + Sync {
    /// 添加新记忆（别名，等同于 save）。
    async fn add(&self, memory: &Memory) -> Result<()> {
        self.save(memory).await
    }

    async fn save(&self, memory: &Memory) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<Memory>>;
    async fn update(&self, memory: &Memory) -> Result<()>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;

    async fn list(&self, limit: usize) -> Result<Vec<Memory>>;
    async fn list_by_type(&self, memory_type: MemoryType, limit: usize) -> Result<Vec<Memory>>;
    async fn list_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Memory>>;

    async fn list_by_importance(&self, min: f32, max: f32) -> Result<Vec<Memory>>;
    async fn list_deleted(&self) -> Result<Vec<Memory>>;

    async fn count(&self) -> Result<usize>;
    async fn count_deleted(&self) -> Result<usize>;

    async fn list_by_project(&self, project_id: &Uuid) -> Result<Vec<Memory>>;
    async fn get_chunks_by_group(&self, chunk_group_id: &Uuid) -> Result<Vec<Memory>>;

    /// 列出创建时间早于指定时间的记忆（用于 Dream 整合）。
    async fn list_older_than(&self, cutoff: &str, limit: usize) -> Result<Vec<Memory>>;
}

/// 项目存储 trait。
#[async_trait]
pub trait ProjectStorage: Send + Sync {
    async fn save(&self, project: &memrec_common::Project) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<memrec_common::Project>>;
    async fn get_by_name(&self, name: &str) -> Result<Option<memrec_common::Project>>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;

    async fn list(&self) -> Result<Vec<memrec_common::Project>>;
}

/// 配置存储 trait（键值对）。
#[async_trait]
pub trait ConfigStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> Result<()>;
}

/// 向量搜索附加载荷，存储在向量索引中用于过滤和结果展示。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VectorPayload {
    pub project_id: Option<Uuid>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub content_preview: String,
    pub importance: f32,
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
}

/// 语义搜索过滤条件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilter {
    pub project_id: Option<Uuid>,
    pub include_global: bool,
    pub memory_type: Option<String>,
    pub min_score: f32,
}

/// 语义搜索命中结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub memory_id: Uuid,
    pub score: f32,
    pub payload: VectorPayload,
}

/// 向量存储 trait。
///
/// 提供向量的增删查和余弦相似度搜索。
#[async_trait]
pub trait VectorStorage: Send + Sync {
    async fn add(&self, id: &Uuid, embedding: &[f32], payload: VectorPayload) -> Result<()>;
    async fn remove(&self, id: &Uuid) -> Result<bool>;
    async fn search(
        &self,
        query: &[f32],
        filter: SearchFilter,
        top_k: usize,
    ) -> Result<Vec<SearchHit>>;
    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>>;
    async fn count(&self) -> Result<usize>;
}

/// 全文搜索附加载荷。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FtsPayload {
    pub project_id: Option<Uuid>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub importance: f32,
}

/// 全文搜索存储 trait。
///
/// 提供 BM25 全文检索功能。
#[async_trait]
pub trait FtsStorage: Send + Sync {
    /// 添加文档到全文索引。
    async fn add(&self, id: &Uuid, text: &str, payload: FtsPayload) -> Result<()>;

    /// 从全文索引中删除文档。
    async fn remove(&self, id: &Uuid) -> Result<bool>;

    /// 全文搜索（BM25）。
    async fn search(
        &self,
        query: &str,
        filter: SearchFilter,
        top_k: usize,
    ) -> Result<Vec<SearchHit>>;

    /// 获取索引文档数量。
    async fn count(&self) -> Result<usize>;

    /// 重载读取器，用于测试场景。默认不做任何操作。
    fn reload(&self) -> Result<()> {
        Ok(())
    }
}

/// 混合搜索请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchRequest {
    pub query: String,
    pub query_embedding: Vec<f32>,
    pub filter: SearchFilter,
    pub top_k: usize,
    /// 向量检索权重（0.0-1.0），默认 0.5
    pub hybrid_alpha: f32,
    /// MMR 相关性权重（0.0-1.0），默认 0.5
    pub mmr_lambda: f32,
    /// 是否启用 MMR 重排
    pub mmr_enabled: bool,
}

/// 混合搜索结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    pub hits: Vec<SearchHit>,
    pub vec_count: usize,
    pub fts_count: usize,
}

/// 混合搜索存储 trait。
///
/// 整合向量检索和全文检索，提供统一的搜索接口。
#[async_trait]
pub trait HybridStorage: Send + Sync {
    /// 混合搜索（KNN + BM25）。
    async fn search(&self, req: HybridSearchRequest) -> Result<HybridSearchResult>;

    /// 添加记忆到索引（向量 + 全文）。
    async fn add(
        &self,
        id: &Uuid,
        embedding: &[f32],
        text: &str,
        payload: VectorPayload,
    ) -> Result<()>;

    /// 从索引中删除记忆。
    async fn remove(&self, id: &Uuid) -> Result<bool>;
}

/// Facet 搜索命中结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacetSearchHit {
    pub facet_id: Uuid,
    pub memory_id: Uuid,
    pub score: f32,
    pub theme: String,
    pub confidence: f32,
    pub keywords: Vec<String>,
}

/// Facet 存储_trait。
///
/// 提供 Facet 的 CRUD、按记忆查询、向量检索等功能。
#[async_trait]
pub trait FacetStorage: Send + Sync {
    async fn save(&self, facet: &MemoryFacet) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<MemoryFacet>>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;

    async fn list_by_memory(&self, memory_id: &Uuid) -> Result<Vec<MemoryFacet>>;
    async fn count(&self) -> Result<usize>;

    async fn search_by_theme(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<FacetSearchHit>>;
}

/// 图遍历扩散参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphTraversalParams {
    pub max_depth: usize,
    pub decay: f32,
    pub min_score: f32,
    pub max_results: usize,
    pub edge_types: Option<Vec<memrec_common::EdgeType>>,
}

impl Default for GraphTraversalParams {
    fn default() -> Self {
        Self {
            max_depth: 3,
            decay: 0.6,
            min_score: 0.3,
            max_results: 50,
            edge_types: None,
        }
    }
}

/// 图扩散命中结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSearchHit {
    pub memory_id: Uuid,
    pub score: f32,
    pub path: Vec<Uuid>,
    pub path_types: Vec<memrec_common::EdgeType>,
}

/// 边存储 trait。
///
/// 提供记忆图边的 CRUD、邻接查询、图遍历扩散等功能。
#[async_trait]
pub trait EdgeStorage: Send + Sync {
    async fn save(&self, edge: &MemoryEdge) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<MemoryEdge>>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;

    async fn list_out_edges(&self, source_id: &Uuid) -> Result<Vec<MemoryEdge>>;
    async fn list_in_edges(&self, target_id: &Uuid) -> Result<Vec<MemoryEdge>>;
    async fn list_by_type(&self, edge_type: memrec_common::EdgeType) -> Result<Vec<MemoryEdge>>;
    async fn count(&self) -> Result<usize>;

    async fn neighbors(&self, memory_id: &Uuid) -> Result<Vec<MemoryEdge>>;

    async fn traverse(
        &self,
        seed_ids: &[Uuid],
        params: GraphTraversalParams,
    ) -> Result<Vec<GraphSearchHit>>;
}

/// 传播存储 trait。
///
/// 提供 PropagationDelta 的 CRUD、队列管理、幂等检查等功能。
#[async_trait]
pub trait PropagationStorage: Send + Sync {
    /// 保存传播增量。
    async fn save(&self, delta: &PropagationDelta) -> Result<()>;

    /// 获取传播增量。
    async fn get(&self, id: &Uuid) -> Result<Option<PropagationDelta>>;

    /// 删除传播增量。
    async fn delete(&self, id: &Uuid) -> Result<bool>;

    /// 列出目标记忆的所有待处理增量（按优先级排序）。
    async fn list_pending(&self, target_id: &Uuid) -> Result<Vec<PropagationDelta>>;

    /// 列出源记忆发出的所有增量。
    async fn list_by_source(&self, source_id: &Uuid) -> Result<Vec<PropagationDelta>>;

    /// 标记增量已应用。
    async fn mark_applied(&self, id: &Uuid) -> Result<bool>;

    /// 检查是否存在已应用的增量（幂等检查）。
    async fn has_applied(&self, source_id: &Uuid, target_id: &Uuid) -> Result<bool>;

    /// 获取待处理增量总数。
    async fn count_pending(&self) -> Result<usize>;

    /// 批量保存增量。
    async fn save_batch(&self, deltas: &[PropagationDelta]) -> Result<()> {
        for delta in deltas {
            self.save(delta).await?;
        }
        Ok(())
    }
}
