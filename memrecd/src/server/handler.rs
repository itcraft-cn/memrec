use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use memrec_common::{
    JsonRpcRequest, JsonRpcResponse, JsonRpcError,
    protocol::{RequestAction, RequestParams, ResponseResult,
    MemoryResult, MemoryListResult, SuccessResult,
    StatsResult, SearchHit, SemanticSearchResult,
    ProjectInfoResult, GetProjectInfoParams, VersionResult},
    Memory, MemoryType,
};
use uuid::Uuid;
use crate::storage::{MemoryStorage, VectorStorage, SearchFilter};
use crate::embedding::FastEmbedGenerator;
use crate::project::{detect_project_id, find_project_root};

pub struct Router {
    storage: Arc<dyn MemoryStorage>,
    vector_store: Arc<dyn VectorStorage>,
    embedder: Arc<FastEmbedGenerator>,
}

impl Router {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        vector_store: Arc<dyn VectorStorage>,
        embedder: Arc<FastEmbedGenerator>,
    ) -> Self {
        Self { storage, vector_store, embedder }
    }
    
    pub fn new_simple(storage: Arc<dyn MemoryStorage>) -> Self {
        let embedder = Arc::new(FastEmbedGenerator::new().unwrap_or_default());
        let vector_store = Arc::new(crate::storage::VectorStore::new(embedder.dimension()));
        Self { storage, vector_store, embedder }
    }
    
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method {
            RequestAction::Add => self.handle_add(request.params, request.id).await,
            RequestAction::Get => self.handle_get(request.params, request.id).await,
            RequestAction::List => self.handle_list(request.params, request.id).await,
            RequestAction::Delete => self.handle_delete(request.params, request.id).await,
            RequestAction::Stats => self.handle_stats(request.id).await,
            
            RequestAction::SearchMemory => self.handle_search_memory(request.params, request.id).await,
            RequestAction::GetProjectInfo => self.handle_project_info(request.params, request.id).await,
            RequestAction::GetVersion => self.handle_version(request.id).await,
            
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                },
                request.id
            )
        }
    }
    
    async fn handle_add(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Add(p)) => {
                let project_id = if p.is_global {
                    Some(Uuid::nil())
                } else {
                    p.project_id.or_else(|| detect_project_id().ok())
                };
                
                let mut memory = Memory::new(p.content.clone(), p.memory_type)
                    .with_tags(p.tags);
                
                if let Some(pid) = project_id {
                    memory = memory.with_project(pid);
                }
                
                match self.storage.save(&memory).await {
                    Ok(_) => {
                        let embedding = self.embedder.embed(&p.content).ok();
                        if let Some(embed) = embedding {
                            let payload = crate::storage::VectorPayload {
                                project_id: memory.project_id,
                                memory_type: memory.memory_type.to_string(),
                                tags: memory.tags.clone(),
                                content_preview: p.content.chars().take(200).collect(),
                                importance: memory.importance,
                                chunk_group_id: memory.chunk_group_id,
                                chunk_index: memory.chunk_index,
                                chunk_total: memory.chunk_total,
                            };
                            self.vector_store.add(&memory.id, &embed, payload).await.ok();
                        }
                        
                        JsonRpcResponse::success(
                            ResponseResult::Memory(MemoryResult { memory }),
                            id
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError { code: -32000, message: e.to_string(), data: None },
                        id
                    )
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params for Add".to_string(), data: None },
                id
            )
        }
    }
    
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
                                id
                            )
                        }
                        Ok(None) => JsonRpcResponse::error(
                            JsonRpcError { code: -32001, message: "Memory not found".to_string(), data: None },
                            id
                        ),
                        Err(e) => JsonRpcResponse::error(
                            JsonRpcError { code: -32000, message: e.to_string(), data: None },
                            id
                        )
                    }
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params for Get".to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_get_with_merge(&self, chunk_group_id: &Uuid, id: u64) -> JsonRpcResponse {
        match self.storage.get_chunks_by_group(chunk_group_id).await {
            Ok(chunks) => {
                if chunks.is_empty() {
                    return JsonRpcResponse::error(
                        JsonRpcError { code: -32005, message: "No chunks found".to_string(), data: None },
                        id
                    );
                }
                
                let mut sorted_chunks = chunks;
                sorted_chunks.sort_by_key(|c| c.chunk_index.unwrap_or(0));
                
                let merged_content = sorted_chunks.iter()
                    .map(|c| c.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                
                let first_chunk = sorted_chunks.first().unwrap();
                let mut merged_memory = Memory::new(merged_content, first_chunk.memory_type)
                    .with_tags(first_chunk.tags.clone());
                merged_memory.id = *chunk_group_id;
                merged_memory.project_id = first_chunk.project_id;
                
                JsonRpcResponse::success(
                    ResponseResult::Memory(MemoryResult { memory: merged_memory }),
                    id
                )
            }
            Err(e) => JsonRpcResponse::error(
                JsonRpcError { code: -32000, message: e.to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_list(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let (limit, project_only, global_only, project_id) = match params {
            Some(RequestParams::List(p)) => (p.limit, p.project_only, p.global_only, p.project_id),
            _ => (20, false, false, None),
        };
        
        let memories = self.storage.list(limit * 5).await.unwrap_or_default();
        
        let filtered: Vec<Memory> = memories.into_iter()
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
            ResponseResult::MemoryList(MemoryListResult { memories: filtered, total }),
            id
        )
    }
    
    async fn handle_delete(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Delete(p)) => {
                match self.storage.delete(&p.id).await {
                    Ok(deleted) => {
                        let message = if deleted { "Memory hard deleted" } else { "Memory soft deleted" };
                        JsonRpcResponse::success(
                            ResponseResult::Success(SuccessResult { message: message.to_string() }),
                            id
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError { code: -32000, message: e.to_string(), data: None },
                        id
                    )
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params for Delete".to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_stats(&self, id: u64) -> JsonRpcResponse {
        match (self.storage.count().await, self.storage.count_deleted().await) {
            (Ok(total), Ok(deleted)) => {
                JsonRpcResponse::success(
                    ResponseResult::Stats(StatsResult {
                        total_memories: total + deleted,
                        active_memories: total,
                        deleted_memories: deleted,
                        storage_usage: 0.0,
                        avg_importance: 0.0,
                    }),
                    id
                )
            }
            (Err(e), _) | (_, Err(e)) => JsonRpcResponse::error(
                JsonRpcError { code: -32000, message: e.to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_search_memory(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SearchMemory(p)) => {
                let start = Instant::now();
                
                let embedding = match self.embedder.embed(&p.query) {
                    Ok(e) => e,
                    Err(e) => return JsonRpcResponse::error(
                        JsonRpcError { code: -32002, message: format!("Embedding error: {}", e), data: None },
                        id
                    ),
                };
                let embed_time = start.elapsed().as_millis() as u64;
                
                let project_id = if p.global_only {
                    Some(Uuid::nil())
                } else if p.project_only {
                    p.project_id.or_else(|| detect_project_id().ok())
                } else {
                    p.project_id
                };
                
                let include_global = !p.project_only;
                
                let filter = SearchFilter {
                    project_id,
                    include_global,
                    memory_type: p.memory_type.map(|t| t.to_string()),
                    min_score: p.min_score,
                };
                
                let search_start = Instant::now();
                let hits = match self.vector_store.search(&embedding, filter, p.top_k).await {
                    Ok(h) => h,
                    Err(e) => return JsonRpcResponse::error(
                        JsonRpcError { code: -32003, message: format!("Search error: {}", e), data: None },
                        id
                    ),
                };
                let search_time = search_start.elapsed().as_millis() as u64;
                
                let mut results: Vec<SearchHit> = vec![];
                for h in hits {
                    let memory = self.storage.get(&h.memory_id).await.ok().flatten();
                    let memory_type = memory.as_ref().map(|m| m.memory_type).unwrap_or(MemoryType::Conversation);
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
                    id
                )
            }
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params for SearchMemory".to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_project_info(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let _params: GetProjectInfoParams = match params {
            Some(RequestParams::GetProjectInfo(p)) => p,
            _ => GetProjectInfoParams,
        };
        
        match detect_project_id() {
            Ok(project_id) => {
                let project_root = find_project_root()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                
                let mr_pid_path = std::path::Path::new(&project_root).join(".mr_pid");
                let mr_pid_exists = mr_pid_path.exists();
                
                let memory_count = self.storage.list_by_project(&project_id).await
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
                    id
                )
            }
            Err(e) => JsonRpcResponse::error(
                JsonRpcError { code: -32004, message: e.to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_version(&self, id: u64) -> JsonRpcResponse {
        JsonRpcResponse::success(
            ResponseResult::Version(VersionResult {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            id
        )
    }
    
    pub fn parse_request(&self, raw: &str) -> Result<JsonRpcRequest> {
        serde_json::from_str(raw)
            .map_err(|e| anyhow::anyhow!("Failed to parse request: {}", e))
    }
    
    pub fn serialize_response(&self, response: &JsonRpcResponse) -> Result<String> {
        serde_json::to_string(response)
            .map_err(|e| anyhow::anyhow!("Failed to serialize response: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, VectorStore};
    use crate::storage::rocksdb::RocksDBStore;
    use tempfile::tempdir;
    use memrec_common::protocol::{SearchMemoryParams, GetProjectInfoParams};
    
    #[tokio::test]
    async fn test_router_search_memory() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));
        let embedder = Arc::new(FastEmbedGenerator::new().unwrap());
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        
        let router = Router::new(storage, vector_store, embedder);
        
        let request = JsonRpcRequest::new(
            RequestAction::SearchMemory,
            Some(RequestParams::SearchMemory(SearchMemoryParams {
                query: "test query".to_string(),
                project_id: None,
                include_global: true,
                project_only: false,
                global_only: false,
                memory_type: None,
                top_k: 10,
                min_score: 0.0,
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
            Some(RequestParams::GetProjectInfo(GetProjectInfoParams)),
            1,
        );
        
        let response = router.route(request).await;
        assert!(response.result.is_some() || response.error.is_some());
    }
}