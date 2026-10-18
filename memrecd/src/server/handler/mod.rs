//! # JSON-RPC 请求路由器
//!
//! [`Router`] 是 memrecd 的请求分发核心，将 JSON-RPC 2.0 请求
//! 路由到对应的处理器方法。
//!
//! ## 支持的方法
//!
//! | 方法 | 说明 |
//! |------|------|
//! | `Add` | 添加记忆（自动生成嵌入向量） |
//! | `Get` | 获取记忆（支持分块合并） |
//! | `List` | 列出记忆（支持项目/全局过滤） |
//! | `Delete` | 删除记忆（软删除/硬删除） |
//! | `Stats` | 统计信息 |
//! | `SearchMemory` | 语义搜索（嵌入+向量检索） |
//! | `GetProjectInfo` | 项目信息 |
//! | `GetVersion` | 版本号 |

use anyhow::Result;
use mr_ability::archive::ArchiveManager;
use mr_ability::dedup::{DedupAction, DedupDecider, DedupDecision};
use mr_ability::dream::DreamProcessor;
use mr_ability::embedding::EmbeddingGenerator;
use mr_ability::project::{detect_project_id, find_project_root};
use mr_ability::storage::{
    DreamRunStorage, HybridSearchRequest, HybridStorage, MemoryStorage, SceneStorage, SearchFilter,
    VectorPayload, VectorStorage,
};
use mr_ability::tiered::TierAssigner;
use mr_common::{
    DedupConfig, Memory, MemoryType, RepresentationTier, SimilarMemory, TierThresholds,
};
use mr_protocol::{
    DreamReportResult, DreamResult, GetProjectInfoParams, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse, MemoryListResult, MemoryResult, ProjectInfoResult, RequestAction,
    RequestParams, ResponseResult, SceneListResult, SceneResult, SearchHit, SemanticSearchResult,
    StatsResult, SuccessResult, VersionResult,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

/// JSON-RPC 请求路由器。
///
/// 持有存储、向量存储、混合搜索存储、嵌入生成器和 Dream 处理器的共享引用，
/// 将请求分发到对应的处理方法。
pub struct Router {
    storage: Arc<dyn MemoryStorage>,
    #[allow(dead_code)]
    vector_store: Arc<dyn VectorStorage>,
    hybrid_store: Arc<dyn HybridStorage>,
    embedder: Arc<dyn EmbeddingGenerator>,
    dream_processor: Arc<DreamProcessor>,
    archive_manager: ArchiveManager,
    scene_store: Arc<dyn SceneStorage>,
    /// 写入去重配置
    dedup_config: DedupConfig,
    /// 去重决策器（规则版；LLM 版预留）
    dedup_decider: Option<Arc<dyn DedupDecider>>,
    /// DreamRun 审计存储（用于 dream_report）
    dream_run_store: Option<Arc<dyn DreamRunStorage>>,
    /// 边存储（用于检索图扩散）
    edge_store: Option<Arc<dyn mr_ability::storage::EdgeStorage>>,
    /// 检索图扩散参数
    retrieval_graph: RetrievalGraph,
}

/// 检索图扩散参数（来自 DreamConfig 的 `retrieval_*` 项）。
#[derive(Debug, Clone, Copy)]
pub struct RetrievalGraph {
    pub enabled: bool,
    pub depth: usize,
    pub decay: f32,
    pub weight: f32,
    /// 图命中保留名额比例（0.0-1.0）：最终 top_k 中为图命中预留的比例。
    pub quota: f32,
}

impl Default for RetrievalGraph {
    fn default() -> Self {
        Self {
            enabled: true,
            depth: 1,
            decay: 0.6,
            weight: 0.3,
            quota: 0.25,
        }
    }
}

impl Router {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        vector_store: Arc<dyn VectorStorage>,
        hybrid_store: Arc<dyn HybridStorage>,
        embedder: Arc<dyn EmbeddingGenerator>,
        dream_processor: Arc<DreamProcessor>,
        archive_manager: ArchiveManager,
        scene_store: Arc<dyn SceneStorage>,
        dedup_config: DedupConfig,
        dedup_decider: Option<Arc<dyn DedupDecider>>,
    ) -> Self {
        Self {
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
            dedup_config,
            dedup_decider,
            dream_run_store: None,
            edge_store: None,
            retrieval_graph: RetrievalGraph::default(),
        }
    }

    /// 注入 DreamRun 审计存储（用于 dream_report 查询）。
    pub fn with_dream_run_store(mut self, store: Arc<dyn DreamRunStorage>) -> Self {
        self.dream_run_store = Some(store);
        self
    }

    /// 注入边存储（用于检索图扩散）。
    pub fn with_edge_store(mut self, store: Arc<dyn mr_ability::storage::EdgeStorage>) -> Self {
        self.edge_store = Some(store);
        self
    }

    /// 配置检索图扩散参数。
    pub fn with_retrieval_graph(mut self, graph: RetrievalGraph) -> Self {
        self.retrieval_graph = graph;
        self
    }

    /// 测试用：仅注入存储，自动创建 MockEmbedder 和内存向量存储。
    #[cfg(test)]
    pub fn new_simple(storage: Arc<dyn MemoryStorage>) -> Self {
        use mr_ability::embedding::MockEmbedder;
        use mr_ability::search::{MmrConfig, ScorerConfig};
        use mr_ability::storage::{HybridStore, SceneStore};
        use mr_common::DreamConfig;

        let embedder = Arc::new(MockEmbedder::new());
        let vector_store = Arc::new(mr_ability::storage::VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(mr_ability::storage::TantivyStore::new_test());
        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));
        let dream_config = DreamConfig::default();
        let data_dir = std::path::PathBuf::from("/tmp/memrec_test");
        let dream_processor = Arc::new(DreamProcessor::new(
            dream_config,
            &data_dir,
            storage.clone(),
        ));
        let archive_manager = ArchiveManager::new(data_dir.clone(), true);

        // scene 存储使用独立临时目录，避免与调用方 storage 的 RocksDB 锁冲突
        let scene_dir = tempfile::tempdir().unwrap();
        let scene_path = scene_dir.path().to_path_buf();
        std::mem::forget(scene_dir);
        let rocksdb = mr_ability::storage::RocksDBStore::open(&scene_path).unwrap();
        let scene_store = Arc::new(SceneStore::new(std::sync::Arc::new(rocksdb)));

        Self {
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
            dedup_config: DedupConfig::default(),
            dedup_decider: None,
            dream_run_store: None,
            edge_store: None,
            retrieval_graph: RetrievalGraph::default(),
        }
    }

    /// 路由 JSON-RPC 请求到对应处理器。
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method {
            RequestAction::Add => self.handle_add(request.params, request.id).await,
            RequestAction::Get => self.handle_get(request.params, request.id).await,
            RequestAction::List => self.handle_list(request.params, request.id).await,
            RequestAction::Delete => self.handle_delete(request.params, request.id).await,
            RequestAction::Restore => self.handle_restore(request.params, request.id).await,
            RequestAction::Stats => self.handle_stats(request.id).await,

            RequestAction::SearchMemory => {
                self.handle_search_memory(request.params, request.id).await
            }
            RequestAction::TieredSearch => {
                self.handle_tiered_search(request.params, request.id).await
            }
            RequestAction::GetProjectInfo => {
                self.handle_project_info(request.params, request.id).await
            }
            RequestAction::GetVersion => self.handle_version(request.id).await,
            RequestAction::Dream => self.handle_dream(request.params, request.id).await,
            RequestAction::DreamReport => {
                self.handle_dream_report(request.params, request.id).await
            }

            RequestAction::SceneCreate => {
                self.handle_scene_create(request.params, request.id).await
            }
            RequestAction::SceneGet => self.handle_scene_get(request.params, request.id).await,
            RequestAction::SceneList => self.handle_scene_list(request.params, request.id).await,
            RequestAction::SceneDelete => {
                self.handle_scene_delete(request.params, request.id).await
            }
            RequestAction::SceneAddMemory => {
                self.handle_scene_add_memory(request.params, request.id)
                    .await
            }
            RequestAction::SceneRemoveMemory => {
                self.handle_scene_remove_memory(request.params, request.id)
                    .await
            }
            RequestAction::SceneUpdateHeat => {
                self.handle_scene_update_heat(request.params, request.id)
                    .await
            }

            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                },
                request.id,
            ),
        }
    }

    /// 解析原始 JSON-RPC 请求字符串。
    pub fn parse_request(&self, raw: &str) -> Result<JsonRpcRequest> {
        serde_json::from_str(raw).map_err(|e| anyhow::anyhow!("Failed to parse request: {}", e))
    }

    /// 序列化 JSON-RPC 响应为字符串。
    pub fn serialize_response(&self, response: &JsonRpcResponse) -> Result<String> {
        serde_json::to_string(response)
            .map_err(|e| anyhow::anyhow!("Failed to serialize response: {}", e))
    }
}

// 子模块：按 JSON-RPC 领域拆分（原单文件约 2700 行）。
mod dream;
mod memory_read;
mod memory_write;
mod scene;
mod search;

#[cfg(test)]
mod tests;
