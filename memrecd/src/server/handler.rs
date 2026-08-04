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
use mr_ability::dream::DreamProcessor;
use mr_ability::embedding::EmbeddingGenerator;
use mr_ability::project::{detect_project_id, find_project_root};
use mr_ability::storage::{
    HybridSearchRequest, HybridStorage, MemoryStorage, SceneStorage, SearchFilter, VectorPayload,
    VectorStorage,
};
use mr_ability::tiered::TierAssigner;
use mr_common::{Memory, MemoryType, RepresentationTier, TierThresholds};
use mr_protocol::{
    DreamResult, GetProjectInfoParams, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    MemoryListResult, MemoryResult, ProjectInfoResult, RequestAction, RequestParams,
    ResponseResult, SceneListResult, SceneResult, SearchHit, SemanticSearchResult, StatsResult,
    SuccessResult, VersionResult,
};
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
}

impl Router {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        vector_store: Arc<dyn VectorStorage>,
        hybrid_store: Arc<dyn HybridStorage>,
        embedder: Arc<dyn EmbeddingGenerator>,
        dream_processor: Arc<DreamProcessor>,
        archive_manager: ArchiveManager,
        scene_store: Arc<dyn SceneStorage>,
    ) -> Self {
        Self {
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
        }
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
        
        let rocksdb = mr_ability::storage::RocksDBStore::open(&data_dir).unwrap();
        let scene_store = Arc::new(SceneStore::new(rocksdb));
        
        Self {
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
        }
    }

    /// 路由 JSON-RPC 请求到对应处理器。
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method {
            RequestAction::Add => self.handle_add(request.params, request.id).await,
            RequestAction::Get => self.handle_get(request.params, request.id).await,
            RequestAction::List => self.handle_list(request.params, request.id).await,
            RequestAction::Delete => self.handle_delete(request.params, request.id).await,
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

            RequestAction::SceneCreate => {
                self.handle_scene_create(request.params, request.id).await
            }
            RequestAction::SceneGet => self.handle_scene_get(request.params, request.id).await,
            RequestAction::SceneList => self.handle_scene_list(request.params, request.id).await,
            RequestAction::SceneDelete => {
                self.handle_scene_delete(request.params, request.id).await
            }
            RequestAction::SceneAddMemory => {
                self.handle_scene_add_memory(request.params, request.id).await
            }
            RequestAction::SceneRemoveMemory => {
                self.handle_scene_remove_memory(request.params, request.id).await
            }
            RequestAction::SceneUpdateHeat => {
                self.handle_scene_update_heat(request.params, request.id).await
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

    /// 处理 Add 请求：保存记忆并生成嵌入向量。
    ///
    /// 全局记忆使用 nil UUID 作为项目 ID；项目记忆自动检测项目 ID。
    async fn handle_add(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Add(p)) => {
                let project_id = if p.is_global {
                    Some(Uuid::nil())
                } else {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                };

                let mut memory = Memory::new(p.content.clone(), p.memory_type).with_tags(p.tags);

                if let Some(pid) = project_id {
                    memory = memory.with_project(pid);
                }

                if let Some(ref s) = p.source {
                    if let Ok(source) = s.parse() {
                        memory = memory.with_source(source);
                    }
                }
                if let Some(ref s) = p.scope {
                    if let Ok(scope) = s.parse() {
                        memory = memory.with_scope(scope);
                    }
                }

                match self.storage.save(&memory).await {
                    Ok(_) => {
                        self.archive_manager.archive(&memory).await;

                        match self.embedder.embed(&p.content) {
                            Ok(embed) => {
                                let payload = VectorPayload {
                                    project_id: memory.project_id,
                                    memory_type: memory.memory_type.to_string(),
                                    tags: memory.tags.clone(),
                                    content_preview: p.content.chars().take(200).collect(),
                                    importance: memory.importance,
                                    chunk_group_id: memory.chunk_group_id,
                                    chunk_index: memory.chunk_index,
                                    chunk_total: memory.chunk_total,
                                };
                                self.hybrid_store
                                    .add(&memory.id, &embed, &p.content, payload)
                                    .await
                                    .ok();
                            }
                            Err(e) => {
                                tracing::warn!("Failed to generate embedding: {}", e);
                            }
                        }

                        JsonRpcResponse::success(
                            ResponseResult::Memory(MemoryResult { memory }),
                            id,
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: e.to_string(),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for Add".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 Get 请求：获取单条记忆。
    ///
    /// 若 `merge=true` 且记忆为分块记忆，合并所有分块返回完整内容。
    async fn handle_get(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Get(p)) => {
                if p.merge {
                    self.handle_get_with_merge(&p.id, id).await
                } else {
                    match self.storage.get(&p.id).await {
                        Ok(Some(memory)) => {
                            self.storage.update(&memory).await.ok();
                            JsonRpcResponse::success(
                                ResponseResult::Memory(MemoryResult { memory }),
                                id,
                            )
                        }
                        Ok(None) => JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32001,
                                message: "Memory not found".to_string(),
                                data: None,
                            },
                            id,
                        ),
                        Err(e) => JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32000,
                                message: e.to_string(),
                                data: None,
                            },
                            id,
                        ),
                    }
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for Get".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 Get 请求（合并模式）：将分块记忆按 chunk_index 排序后拼接。
    async fn handle_get_with_merge(&self, id: &Uuid, rpc_id: u64) -> JsonRpcResponse {
        match self.storage.get(id).await {
            Ok(Some(memory)) => {
                if !memory.is_chunked() {
                    self.storage.update(&memory).await.ok();
                    return JsonRpcResponse::success(
                        ResponseResult::Memory(MemoryResult { memory }),
                        rpc_id,
                    );
                }
                let group_id = memory.chunk_group_id.unwrap();
                match self.storage.get_chunks_by_group(&group_id).await {
                    Ok(chunks) => {
                        if chunks.is_empty() {
                            return JsonRpcResponse::error(
                                JsonRpcError {
                                    code: -32005,
                                    message: "No chunks found".to_string(),
                                    data: None,
                                },
                                rpc_id,
                            );
                        }
                        let mut sorted_chunks = chunks;
                        sorted_chunks.sort_by_key(|c| c.chunk_index.unwrap_or(0));
                        let merged_content = sorted_chunks
                            .iter()
                            .map(|c| c.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let first_chunk = sorted_chunks.first().unwrap();
                        let mut merged_memory =
                            Memory::new(merged_content, first_chunk.memory_type)
                                .with_tags(first_chunk.tags.clone());
                        merged_memory.id = group_id;
                        merged_memory.project_id = first_chunk.project_id;
                        JsonRpcResponse::success(
                            ResponseResult::Memory(MemoryResult {
                                memory: merged_memory,
                            }),
                            rpc_id,
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: e.to_string(),
                            data: None,
                        },
                        rpc_id,
                    ),
                }
            }
            Ok(None) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32001,
                    message: "Memory not found".to_string(),
                    data: None,
                },
                rpc_id,
            ),
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32000,
                    message: e.to_string(),
                    data: None,
                },
                rpc_id,
            ),
        }
    }

    /// 处理 List 请求：列出记忆，支持项目/全局过滤。
    async fn handle_list(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let (limit, project_only, global_only, project_id) = match params {
            Some(RequestParams::List(p)) => (p.limit, p.project_only, p.global_only, p.project_id),
            _ => (20, false, false, None),
        };

        let memories = self.storage.list(limit * 5).await.unwrap_or_default();

        let filtered: Vec<Memory> = memories
            .into_iter()
            .filter(|m| {
                if m.is_deleted {
                    return false;
                }

                if project_only {
                    if let Some(pid) = project_id {
                        m.project_id == Some(pid)
                    } else {
                        m.project_id.is_some() && !m.project_id.unwrap().is_nil()
                    }
                } else if global_only {
                    m.project_id.is_none() || m.project_id.unwrap().is_nil()
                } else {
                    true
                }
            })
            .take(limit)
            .collect();

        let total = filtered.len();
        JsonRpcResponse::success(
            ResponseResult::MemoryList(MemoryListResult {
                memories: filtered,
                total,
            }),
            id,
        )
    }

    /// 处理 Delete 请求：删除记忆。
    ///
    /// 返回消息区分硬删除（已过恢复期）和软删除。
    async fn handle_delete(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Delete(p)) => match self.storage.delete(&p.id).await {
                Ok(deleted) => {
                    let message = if deleted {
                        "Memory hard deleted"
                    } else {
                        "Memory soft deleted"
                    };
                    JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: message.to_string(),
                        }),
                        id,
                    )
                }
                Err(e) => JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32000,
                        message: e.to_string(),
                        data: None,
                    },
                    id,
                ),
            },
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for Delete".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 Stats 请求：返回记忆统计信息。
    async fn handle_stats(&self, id: u64) -> JsonRpcResponse {
        match (
            self.storage.count().await,
            self.storage.count_deleted().await,
        ) {
            (Ok(total), Ok(deleted)) => JsonRpcResponse::success(
                ResponseResult::Stats(StatsResult {
                    total_memories: total + deleted,
                    active_memories: total,
                    deleted_memories: deleted,
                    storage_usage: 0.0,
                    avg_importance: 0.0,
                }),
                id,
            ),
            (Err(e), _) | (_, Err(e)) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32000,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SearchMemory 请求：混合搜索（向量 + 全文）。
    ///
    /// 流程：生成查询嵌入 → 混合搜索（KNN + BM25）→ MMR重排 → 补充记忆元数据 → 返回结果。
    /// 返回结果包含嵌入耗时和搜索耗时。
    async fn handle_search_memory(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SearchMemory(p)) => {
                let start = Instant::now();

                let embedding = match self.embedder.embed(&p.query) {
                    Ok(e) => e,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32002,
                                message: format!("Embedding error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };
                let embed_time = start.elapsed().as_millis() as u64;

                let project_id = if p.cross_project {
                    None
                } else if p.global_only {
                    Some(Uuid::nil())
                } else if p.project_only {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                } else {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                };

                let include_global = !p.project_only && !p.cross_project;

                let filter = SearchFilter {
                    project_id,
                    include_global,
                    memory_type: p.memory_type.map(|t| t.to_string()),
                    min_score: p.min_score,
                };

                let search_start = Instant::now();
                let req = HybridSearchRequest {
                    query: p.query.clone(),
                    query_embedding: embedding,
                    filter,
                    top_k: p.top_k,
                    hybrid_alpha: p.hybrid_alpha as f32,
                    mmr_lambda: p.mmr_lambda as f32,
                    mmr_enabled: p.mmr_enabled,
                };

                let result = match self.hybrid_store.search(req).await {
                    Ok(r) => r,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32003,
                                message: format!("Search error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };
                let search_time = search_start.elapsed().as_millis() as u64;

                let mut results: Vec<SearchHit> = vec![];
                for h in result.hits {
                    let memory = self.storage.get(&h.memory_id).await.ok().flatten();
                    let memory_type = memory
                        .as_ref()
                        .map(|m| m.memory_type)
                        .unwrap_or(MemoryType::Conversation);
                    let created_at = memory.map(|m| m.created_at).unwrap_or_default();
                    results.push(SearchHit {
                        memory_id: h.memory_id,
                        score: h.score,
                        memory_type,
                        content_preview: h.payload.content_preview.clone(),
                        project_id: h.payload.project_id,
                        tags: h.payload.tags.clone(),
                        is_chunked: h.payload.chunk_group_id.is_some(),
                        chunk_group_id: h.payload.chunk_group_id,
                        chunk_index: h.payload.chunk_index,
                        chunk_total: h.payload.chunk_total,
                        created_at,
                    });
                }

                let total = results.len();

                JsonRpcResponse::success(
                    ResponseResult::SemanticSearchResult(SemanticSearchResult {
                        results,
                        total,
                        query_embedding_time_ms: embed_time,
                        search_time_ms: search_time,
                    }),
                    id,
                )
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SearchMemory".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    async fn handle_tiered_search(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::TieredSearch(p)) => {
                let start = Instant::now();

                let embedding = match self.embedder.embed(&p.query) {
                    Ok(e) => e,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32002,
                                message: format!("Embedding error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };
                let _embed_time = start.elapsed().as_millis() as u64;

                let project_id = if p.cross_project {
                    None
                } else if p.global_only {
                    Some(Uuid::nil())
                } else if p.project_only {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                } else {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                };

                let include_global = !p.project_only && !p.cross_project;

                let filter = SearchFilter {
                    project_id,
                    include_global,
                    memory_type: p.memory_type.map(|t| t.to_string()),
                    min_score: p.min_score,
                };

                let req = HybridSearchRequest {
                    query: p.query.clone(),
                    query_embedding: embedding,
                    filter,
                    top_k: p.top_k,
                    hybrid_alpha: 0.5,
                    mmr_lambda: 0.5,
                    mmr_enabled: true,
                };

                let result = match self.hybrid_store.search(req).await {
                    Ok(r) => r,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32003,
                                message: format!("Search error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };

                let memory_ids: Vec<Uuid> = result.hits.iter().map(|h| h.memory_id).collect();
                let scores: Vec<f32> = result.hits.iter().map(|h| h.score).collect();

                let thresholds = p
                    .tier_thresholds
                    .as_ref()
                    .map(|t| TierThresholds {
                        full: t.full,
                        truncated: t.truncated,
                        summary: t.summary,
                    })
                    .unwrap_or_default();

                let assigner = TierAssigner::new(thresholds);
                let budget = mr_ability::tiered::BudgetController::new(p.max_total_tokens);

                match self
                    .build_tiered_result(memory_ids, scores, assigner, budget)
                    .await
                {
                    Ok(tiered_result) => JsonRpcResponse::success(
                        ResponseResult::TieredSearchResult(tiered_result),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32005,
                            message: format!("Tiered search error: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for TieredSearch".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 GetProjectInfo 请求：返回项目 ID、根目录和记忆数量。
    async fn handle_project_info(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let params: GetProjectInfoParams = match params {
            Some(RequestParams::GetProjectInfo(p)) => p,
            _ => GetProjectInfoParams::default(),
        };

        match detect_project_id(params.working_dir.as_deref()) {
            Ok(project_id) => {
                let project_root = find_project_root(params.working_dir.as_deref())
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let mr_pid_path = std::path::Path::new(&project_root).join(".mr_pid");
                let mr_pid_exists = mr_pid_path.exists();

                let memory_count = self
                    .storage
                    .list_by_project(&project_id)
                    .await
                    .unwrap_or_default()
                    .len();

                JsonRpcResponse::success(
                    ResponseResult::ProjectInfo(ProjectInfoResult {
                        project_id,
                        project_name: None,
                        project_root,
                        memory_count,
                        mr_pid_exists,
                    }),
                    id,
                )
            }
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32004,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 GetVersion 请求：返回守护进程版本号。
    async fn handle_version(&self, id: u64) -> JsonRpcResponse {
        JsonRpcResponse::success(
            ResponseResult::Version(VersionResult {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            id,
        )
    }

    async fn handle_dream(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let force = match params {
            Some(RequestParams::Dream(p)) => p.force,
            _ => false,
        };

        match self.dream_processor.execute(force).await {
            Ok(result) => JsonRpcResponse::success(
                ResponseResult::Dream(DreamResult {
                    integrated_count: result.integrated_count,
                    created_memory_id: result.created_memory_id,
                    summary: result.summary,
                }),
                id,
            ),
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32006,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    async fn build_tiered_result(
        &self,
        memory_ids: Vec<Uuid>,
        scores: Vec<f32>,
        assigner: TierAssigner,
        budget: mr_ability::tiered::BudgetController,
    ) -> Result<mr_common::TieredSearchResult> {
        use mr_common::TieredMemory;

        let mut results = Vec::with_capacity(memory_ids.len());

        for (memory_id, score) in memory_ids.iter().zip(scores.iter()) {
            if let Some(memory) = self.storage.get(memory_id).await? {
                let tier = assigner.assign(*score);
                let (content, token_count) = Self::generate_tier_content(&memory.content, tier);

                results.push(TieredMemory {
                    memory_id: *memory_id,
                    tier,
                    content,
                    score: *score,
                    token_count,
                    facet_themes: vec![],
                    edge_hints: vec![],
                });
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_tokens = budget.check_and_downgrade(&mut results);

        Ok(mr_common::TieredSearchResult {
            total_results: results.len(),
            total_tokens,
            budget: budget.max_tokens(),
            results,
        })
    }

    fn generate_tier_content(content: &str, tier: RepresentationTier) -> (String, u32) {
        let tokens = mr_ability::tiered::BudgetController::estimate_token_count(content);

        match tier {
            RepresentationTier::Full => (content.to_string(), tokens),
            RepresentationTier::Truncated => {
                let limit = 500;
                let truncated = if content.chars().count() > limit {
                    let chars: Vec<char> = content.chars().take(limit).collect();
                    let truncated: String = chars.into_iter().collect();
                    format!("{}...[truncated]", truncated)
                } else {
                    content.to_string()
                };
                let token_count =
                    mr_ability::tiered::BudgetController::estimate_token_count(&truncated);
                (truncated, token_count.min(200))
            }
            RepresentationTier::Summary => {
                let preview = if content.chars().count() > 100 {
                    let chars: Vec<char> = content.chars().take(100).collect();
                    chars.into_iter().collect()
                } else {
                    content.to_string()
                };
                (format!("Summary: {}", preview), 80)
            }
            RepresentationTier::DenseProxy => {
                let words: Vec<&str> = content.split_whitespace().take(5).collect();
                (format!("[{}]", words.join(", ")), 30)
            }
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

    /// 处理 SceneCreate 请求：创建场景。
    async fn handle_scene_create(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneCreate(p)) => {
                let mut scene = mr_common::Scene::new(p.theme);
                if let Some(project_id) = p.project_id {
                    scene.project_id = Some(project_id);
                }
                scene.tags = p.tags;

                match self.scene_store.save_scene(&scene).await {
                    Ok(_) => JsonRpcResponse::success(ResponseResult::Scene(SceneResult { scene }), id),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to create scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneCreate".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneGet 请求：获取场景。
    async fn handle_scene_get(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneGet(p)) => {
                match self.scene_store.get_scene(&p.id).await {
                    Ok(Some(scene)) => JsonRpcResponse::success(ResponseResult::Scene(SceneResult { scene }), id),
                    Ok(None) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32001,
                            message: "Scene not found".to_string(),
                            data: None,
                        },
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to get scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneGet".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneList 请求：列出场景。
    async fn handle_scene_list(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneList(p)) => {
                match self.scene_store.list_scenes(p.limit, p.sort_by_heat, p.project_id).await {
                    Ok(scenes) => {
                        let total = scenes.len();
                        JsonRpcResponse::success(
                            ResponseResult::SceneList(SceneListResult { scenes, total }),
                            id,
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to list scenes: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneList".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneDelete 请求：删除场景。
    async fn handle_scene_delete(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneDelete(p)) => {
                match self.scene_store.delete_scene(&p.id).await {
                    Ok(true) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Scene deleted".to_string(),
                        }),
                        id,
                    ),
                    Ok(false) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Scene marked as deleted".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to delete scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneDelete".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneAddMemory 请求：添加记忆到场景。
    async fn handle_scene_add_memory(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneAddMemory(p)) => {
                match self.scene_store.add_memory_to_scene(&p.scene_id, &p.memory_id).await {
                    Ok(_) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Memory added to scene".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to add memory to scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneAddMemory".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneRemoveMemory 请求：从场景移除记忆。
    async fn handle_scene_remove_memory(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneRemoveMemory(p)) => {
                match self.scene_store.remove_memory_from_scene(&p.scene_id, &p.memory_id).await {
                    Ok(_) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Memory removed from scene".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to remove memory from scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneRemoveMemory".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneUpdateHeat 请求：更新场景热度。
    async fn handle_scene_update_heat(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneUpdateHeat(p)) => {
                match self.scene_store.update_scene_heat(&p.scene_id, p.heat).await {
                    Ok(_) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Scene heat updated".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to update scene heat: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneUpdateHeat".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_ability::embedding::MockEmbedder;
    use mr_ability::search::{MmrConfig, ScorerConfig};
    use mr_ability::storage::rocksdb::RocksDBStore;
    use mr_ability::storage::{HybridStore, MemoryStore, TantivyStore, VectorStore};
    use mr_common::DreamConfig;
    use mr_protocol::{
        default_hybrid_alpha, default_mmr_enabled, default_mmr_lambda, GetProjectInfoParams,
        SearchMemoryParams,
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_router_search_memory() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));
        let embedder = Arc::new(MockEmbedder::new());
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(TantivyStore::new_test());
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

        let rocksdb_for_scene = RocksDBStore::open(&data_dir).unwrap();
        let scene_store = Arc::new(SceneStore::new(rocksdb_for_scene));

        let router = Router::new(
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
        );

        let request = JsonRpcRequest::new(
            RequestAction::SearchMemory,
            Some(RequestParams::SearchMemory(SearchMemoryParams {
                query: "test query".to_string(),
                project_id: None,
                include_global: true,
                project_only: false,
                global_only: false,
                cross_project: false,
                memory_type: None,
                top_k: 10,
                min_score: 0.0,
                working_dir: None,
                hybrid_alpha: default_hybrid_alpha(),
                mmr_enabled: default_mmr_enabled(),
                mmr_lambda: default_mmr_lambda(),
            })),
            1,
        );

        let response = router.route(request).await;
        assert!(response.result.is_some());
    }

    #[tokio::test]
    async fn test_router_project_info() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let router = Router::new_simple(storage);

        let request = JsonRpcRequest::new(
            RequestAction::GetProjectInfo,
            Some(RequestParams::GetProjectInfo(
                GetProjectInfoParams::default(),
            )),
            1,
        );

        let response = router.route(request).await;
        assert!(response.result.is_some() || response.error.is_some());
    }
}
