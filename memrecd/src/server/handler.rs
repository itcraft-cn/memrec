use anyhow::Result;
use std::sync::Arc;
use memrec_common::{
    JsonRpcRequest, JsonRpcResponse, JsonRpcError,
    protocol::{RequestAction, RequestParams, ResponseResult,
    MemoryResult, MemoryListResult, SuccessResult,
    StatsResult},
};
use crate::storage::MemoryStorage;

pub struct Router {
    storage: Arc<dyn MemoryStorage>,
}

impl Router {
    pub fn new(storage: Arc<dyn MemoryStorage>) -> Self {
        Self { storage }
    }
    
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method {
            RequestAction::Add => {
                self.handle_add(request.params, request.id).await
            }
            RequestAction::Get => {
                self.handle_get(request.params, request.id).await
            }
            RequestAction::List => {
                self.handle_list(request.params, request.id).await
            }
            RequestAction::Delete => {
                self.handle_delete(request.params, request.id).await
            }
            RequestAction::Stats => {
                self.handle_stats(request.id).await
            }
            _ => {
                JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    },
                    request.id
                )
            }
        }
    }
    
    async fn handle_add(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Add(p)) => {
                let memory = memrec_common::Memory::new(p.content, p.memory_type)
                    .with_tags(p.tags);
                
                match self.storage.save(&memory).await {
                    Ok(_) => JsonRpcResponse::success(
                        ResponseResult::Memory(MemoryResult { memory }),
                        id
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError { code: -32000, message: e.to_string(), data: None },
                        id
                    )
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params".to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_get(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Get(p)) => {
                match self.storage.get(&p.id).await {
                    Ok(Some(memory)) => JsonRpcResponse::success(
                        ResponseResult::Memory(MemoryResult { memory }),
                        id
                    ),
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
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params".to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_list(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let limit = match params {
            Some(RequestParams::List(p)) => p.limit,
            _ => 20,
        };
        
        match self.storage.list(limit).await {
            Ok(memories) => {
                let total = memories.len();
                JsonRpcResponse::success(
                    ResponseResult::MemoryList(MemoryListResult { memories, total }),
                    id
                )
            }
            Err(e) => JsonRpcResponse::error(
                JsonRpcError { code: -32000, message: e.to_string(), data: None },
                id
            )
        }
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
                JsonRpcError { code: -32602, message: "Invalid params".to_string(), data: None },
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
    
    pub fn parse_request(&self, raw: &str) -> Result<JsonRpcRequest> {
        serde_json::from_str(raw)
            .map_err(|e| anyhow::anyhow!("Failed to parse request: {}", e))
    }
    
    pub fn serialize_response(&self, response: &JsonRpcResponse) -> Result<String> {
        serde_json::to_string(response)
            .map_err(|e| anyhow::anyhow!("Failed to serialize response: {}", e))
    }
}