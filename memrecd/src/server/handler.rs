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
    HybridSearchRequest, HybridStorage, MemoryStorage, SceneStorage, SearchFilter, VectorPayload,
    VectorStorage,
};
use mr_ability::tiered::TierAssigner;
use mr_common::{
    DedupConfig, Memory, MemoryType, RepresentationTier, SimilarMemory, TierThresholds,
};
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
    /// 写入去重配置
    dedup_config: DedupConfig,
    /// 去重决策器（规则版；LLM 版预留）
    dedup_decider: Option<Arc<dyn DedupDecider>>,
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

    /// 处理 Add 请求：保存记忆并生成嵌入向量。
    ///
    /// 全局记忆使用 nil UUID 作为项目 ID；项目记忆自动检测项目 ID。
    /// 开启去重检测时，先检索相似记忆并按规则决策：
    /// - 重复（相似度 >= duplicate_threshold）→ `skipped`，不写入
    /// - 可更新（>= update_threshold）→ `updated`，合并更新已有记忆
    /// - 否则正常写入 → `added`
    async fn handle_add(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Add(p)) => {
                let project_id = if p.is_global {
                    Some(Uuid::nil())
                } else {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                };

                let mut memory =
                    Memory::new(p.content.clone(), p.memory_type).with_tags(p.tags.clone());

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

                // 去重检测：调用方可通过 dedup=false 显式关闭，默认遵循配置
                let dedup_enabled = p.dedup.unwrap_or(self.dedup_config.enabled);
                if dedup_enabled {
                    if let Some(decision) = self.check_dedup(&p.content).await {
                        match decision.action {
                            DedupAction::Skip => {
                                tracing::info!(
                                    "Add skipped: duplicate of {:?} (score={:.3})",
                                    decision.candidates.first().map(|c| c.memory_id),
                                    decision.candidates.first().map(|c| c.score).unwrap_or(0.0)
                                );
                                return JsonRpcResponse::success(
                                    ResponseResult::Memory(MemoryResult {
                                        memory,
                                        action: "skipped".to_string(),
                                        duplicates: decision.candidates,
                                    }),
                                    id,
                                );
                            }
                            DedupAction::Update(target_id) => {
                                if let Ok(Some(old)) = self.storage.get(&target_id).await {
                                    let mut updated = old.clone();
                                    updated.content = p.content.clone();
                                    for t in &p.tags {
                                        if !updated.tags.contains(t) {
                                            updated.tags.push(t.clone());
                                        }
                                    }
                                    updated.last_accessed = chrono::Utc::now();
                                    updated.access_count += 1;

                                    match self.storage.update(&updated).await {
                                        Ok(_) => {
                                            // 重建向量索引：先删旧向量再写新向量
                                            let _ = self.hybrid_store.remove(&target_id).await;
                                            if let Ok(embed) = self.embedder.embed(&updated.content)
                                            {
                                                let payload = self.build_payload(&updated);
                                                let _ = self
                                                    .hybrid_store
                                                    .add(
                                                        &target_id,
                                                        &embed,
                                                        &updated.content,
                                                        payload,
                                                    )
                                                    .await;
                                            }
                                            self.archive_manager.archive(&updated).await;

                                            tracing::info!(
                                                "Add updated existing memory {:?}",
                                                target_id
                                            );
                                            return JsonRpcResponse::success(
                                                ResponseResult::Memory(MemoryResult {
                                                    memory: updated,
                                                    action: "updated".to_string(),
                                                    duplicates: decision.candidates,
                                                }),
                                                id,
                                            );
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "Add dedup update failed ({}), falling back to add",
                                                e
                                            );
                                        }
                                    }
                                } else {
                                    tracing::warn!(
                                        "Add dedup target {:?} not found, falling back to add",
                                        target_id
                                    );
                                }
                            }
                            DedupAction::Add => {}
                        }
                    }
                }

                match self.storage.save(&memory).await {
                    Ok(_) => {
                        self.archive_manager.archive(&memory).await;

                        match self.embedder.embed(&p.content) {
                            Ok(embed) => {
                                let payload = self.build_payload(&memory);
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
                            ResponseResult::Memory(MemoryResult {
                                memory,
                                action: "added".to_string(),
                                duplicates: Vec::new(),
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

    /// 构造向量索引载荷。
    fn build_payload(&self, memory: &Memory) -> VectorPayload {
        VectorPayload {
            project_id: memory.project_id,
            memory_type: memory.memory_type.to_string(),
            tags: memory.tags.clone(),
            content_preview: memory.content.chars().take(200).collect(),
            importance: memory.importance,
            chunk_group_id: memory.chunk_group_id,
            chunk_index: memory.chunk_index,
            chunk_total: memory.chunk_total,
        }
    }

    /// 写入去重检测：检索相似记忆并做出决策。
    ///
    /// 使用向量检索（余弦相似度）查找相似候选，过滤低于 `update_threshold` 的命中，
    /// 交由决策器（规则版/LLM 版）判断动作。
    /// 注意：此处直接用向量分数而非混合检索——混合检索的融合归一化分数
    /// 会稀释原始相似度，不适合阈值判断。
    async fn check_dedup(&self, content: &str) -> Option<DedupDecision> {
        let decider = self.dedup_decider.as_ref()?;
        if !self.dedup_config.enabled {
            return None;
        }

        let embed = self.embedder.embed(content).ok()?;
        let filter = SearchFilter {
            project_id: None,
            include_global: true,
            memory_type: None,
            min_score: 0.0,
        };
        let hits = self
            .vector_store
            .search(&embed, filter, self.dedup_config.top_k.max(1))
            .await
            .ok()?;

        let candidates: Vec<SimilarMemory> = hits
            .into_iter()
            .filter(|h| h.score >= self.dedup_config.update_threshold)
            .map(|h| SimilarMemory {
                memory_id: h.memory_id,
                content: h.payload.content_preview,
                score: h.score,
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        Some(decider.decide(content, &candidates).await)
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
                                ResponseResult::Memory(MemoryResult::new(memory)),
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
                        ResponseResult::Memory(MemoryResult::new(memory)),
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
                            ResponseResult::Memory(MemoryResult::new(merged_memory)),
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
                    Ok(_) => {
                        JsonRpcResponse::success(ResponseResult::Scene(SceneResult { scene }), id)
                    }
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
            Some(RequestParams::SceneGet(p)) => match self.scene_store.get_scene(&p.id).await {
                Ok(Some(scene)) => {
                    JsonRpcResponse::success(ResponseResult::Scene(SceneResult { scene }), id)
                }
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
            },
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
                match self
                    .scene_store
                    .list_scenes(p.limit, p.sort_by_heat, p.project_id)
                    .await
                {
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
    async fn handle_scene_add_memory(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneAddMemory(p)) => {
                match self
                    .scene_store
                    .add_memory_to_scene(&p.scene_id, &p.memory_id)
                    .await
                {
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
    async fn handle_scene_remove_memory(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneRemoveMemory(p)) => {
                match self
                    .scene_store
                    .remove_memory_from_scene(&p.scene_id, &p.memory_id)
                    .await
                {
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
    async fn handle_scene_update_heat(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneUpdateHeat(p)) => {
                match self
                    .scene_store
                    .update_scene_heat(&p.scene_id, p.heat)
                    .await
                {
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
    use mr_ability::dedup::RuleDedupDecider;
    use mr_ability::embedding::MockEmbedder;
    use mr_ability::search::{MmrConfig, ScorerConfig};
    use mr_ability::storage::rocksdb::RocksDBStore;
    use mr_ability::storage::{HybridStore, MemoryStore, SceneStore, TantivyStore, VectorStore};
    use mr_common::DreamConfig;
    use mr_protocol::{
        default_hybrid_alpha, default_mmr_enabled, default_mmr_lambda, AddParams,
        GetProjectInfoParams, SearchMemoryParams,
    };
    use tempfile::tempdir;

    /// 可预测相似度的测试嵌入器。
    ///
    /// 每个字节映射到独立维度，相同内容得到相同向量（相似度 1.0），
    /// 共享前缀越长相似度越高，便于控制去重测试场景。
    struct TestEmbedder;

    impl mr_ability::embedding::EmbeddingGenerator for TestEmbedder {
        fn dimension(&self) -> usize {
            384
        }

        fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            let mut v = vec![0.0f32; 384];
            for (i, b) in text.bytes().take(384).enumerate() {
                v[i] = b as f32 / 255.0;
            }
            Ok(v)
        }

        fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|t| self.embed(t).unwrap()).collect())
        }
    }

    /// 构造带去重配置的路由器（注入 TestEmbedder + 规则决策器）。
    fn router_with_dedup(dup_threshold: f32, update_threshold: f32) -> Router {
        let dir = tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();
        // 保持临时目录存活（Router 生命周期内 RocksDB 需要访问数据文件）
        std::mem::forget(dir);
        // 共享 RocksDB 实例（MemoryStore 与 SceneStore 复用，避免锁冲突）
        let rocksdb = Arc::new(RocksDBStore::open(&data_dir).unwrap());
        let storage = Arc::new(MemoryStore::new(rocksdb.clone()));
        let embedder = Arc::new(TestEmbedder);
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(TantivyStore::new_test());
        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));

        let dream_config = DreamConfig::default();
        let dream_processor = Arc::new(DreamProcessor::new(
            dream_config,
            &data_dir,
            storage.clone(),
        ));
        let archive_manager = ArchiveManager::new(data_dir.clone(), true);
        let scene_store = Arc::new(SceneStore::new(rocksdb));

        let dedup_config = DedupConfig {
            enabled: true,
            top_k: 5,
            duplicate_threshold: dup_threshold,
            update_threshold,
        };
        let decider: Option<Arc<dyn DedupDecider>> =
            Some(Arc::new(RuleDedupDecider::from_config(&dedup_config)));

        Router::new(
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
            dedup_config,
            decider,
        )
    }

    fn add_request(content: &str, id: u64) -> JsonRpcRequest {
        JsonRpcRequest::new(
            RequestAction::Add,
            Some(RequestParams::Add(AddParams {
                content: content.to_string(),
                memory_type: MemoryType::Knowledge,
                tags: vec![],
                project_id: None,
                is_global: true,
                working_dir: None,
                source: None,
                scope: None,
                dedup: None,
            })),
            id,
        )
    }

    fn add_request_dedup_off(content: &str, id: u64) -> JsonRpcRequest {
        let mut req = add_request(content, id);
        if let Some(RequestParams::Add(p)) = &mut req.params {
            p.dedup = Some(false);
        }
        req
    }

    fn extract_action(response: &JsonRpcResponse) -> String {
        match &response.result {
            Some(ResponseResult::Memory(m)) => m.action.clone(),
            _ => String::new(),
        }
    }

    #[tokio::test]
    async fn test_add_duplicate_skipped() {
        let router = router_with_dedup(0.95, 0.80);
        let content = "jwt auth uses RS256 signing";

        let first = router.route(add_request(content, 1)).await;
        assert_eq!(extract_action(&first), "added");

        let second = router.route(add_request(content, 2)).await;
        assert_eq!(extract_action(&second), "skipped");

        // 记忆库只有一条
        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_add_update_merges_existing() {
        let router = router_with_dedup(0.95, 0.80);
        let base = "jwt auth uses RS256 signing";
        let evolved = "jwt auth uses RS256 signing rotation";

        let first = router.route(add_request(base, 1)).await;
        let first_id = match &first.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };

        let second = router.route(add_request(evolved, 2)).await;
        assert_eq!(extract_action(&second), "updated");
        let updated = match &second.result {
            Some(ResponseResult::Memory(m)) => m.memory.clone(),
            _ => panic!("expected memory result"),
        };
        // 更新保留了原 ID，内容为新内容
        assert_eq!(updated.id, first_id);
        assert!(updated.content.contains("rotation"));
        let duplicates = match &second.result {
            Some(ResponseResult::Memory(m)) => m.duplicates.clone(),
            _ => panic!("expected memory result"),
        };
        assert!(!duplicates.is_empty());

        // 记忆库仍只有一条
        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_add_unrelated_adds() {
        let router = router_with_dedup(0.95, 0.80);

        let first = router.route(add_request("jwt auth uses RS256", 1)).await;
        assert_eq!(extract_action(&first), "added");

        let second = router
            .route(add_request("rust ownership and borrowing", 2))
            .await;
        assert_eq!(extract_action(&second), "added");

        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_add_dedup_explicitly_disabled() {
        let router = router_with_dedup(0.95, 0.80);
        let content = "jwt auth uses RS256 signing";

        let first = router.route(add_request(content, 1)).await;
        assert_eq!(extract_action(&first), "added");

        // dedup=false 显式关闭，重复内容也写入
        let second = router.route(add_request_dedup_off(content, 2)).await;
        assert_eq!(extract_action(&second), "added");

        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_router_search_memory() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));
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

        let rocksdb_for_scene = Arc::new(RocksDBStore::open(&data_dir).unwrap());
        let scene_store = Arc::new(SceneStore::new(rocksdb_for_scene));

        let router = Router::new(
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
            DedupConfig::default(),
            None,
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
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

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
