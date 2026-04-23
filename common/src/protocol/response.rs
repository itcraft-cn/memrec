use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::types::{Memory, MemoryType, Project};
use super::error::JsonRpcError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ResponseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: u64,
}

impl JsonRpcResponse {
    pub fn success(result: ResponseResult, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }
    
    pub fn error(err: JsonRpcError, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(err),
            id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseResult {
    Memory(MemoryResult),
    MemoryList(MemoryListResult),
    SearchResult(SearchResult),
    SemanticSearchResult(SemanticSearchResult),
    Project(ProjectResult),
    ProjectList(ProjectListResult),
    ProjectInfo(ProjectInfoResult),
    Config(ConfigResult),
    Stats(StatsResult),
    Success(SuccessResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub memory: Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListResult {
    pub memories: Vec<Memory>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memories: Vec<Memory>,
    pub total: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    pub results: Vec<SearchHit>,
    pub total: usize,
    pub query_embedding_time_ms: u64,
    pub search_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub memory_id: Uuid,
    pub score: f32,
    pub memory_type: MemoryType,
    pub content_preview: String,
    pub project_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub is_chunked: bool,
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResult {
    pub project: Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectListResult {
    pub projects: Vec<Project>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfoResult {
    pub project_id: Uuid,
    pub project_name: Option<String>,
    pub project_root: String,
    pub memory_count: usize,
    pub mr_pid_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResult {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub total_memories: usize,
    pub active_memories: usize,
    pub deleted_memories: usize,
    pub storage_usage: f32,
    pub avg_importance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResult {
    pub message: String,
}

impl From<bool> for SuccessResult {
    fn from(success: bool) -> Self {
        Self {
            message: if success { "Success".to_string() } else { "Failed".to_string() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryType;
    
    #[test]
    fn test_success_response() {
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        let resp = JsonRpcResponse::success(
            ResponseResult::Memory(MemoryResult { memory }),
            1,
        );
        
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }
    
    #[test]
    fn test_error_response() {
        let err = JsonRpcError {
            code: -32001,
            message: "Not found".to_string(),
            data: None,
        };
        let resp = JsonRpcResponse::error(err, 1);
        
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }
    
    #[test]
    fn test_response_serde() {
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        let resp = JsonRpcResponse::success(
            ResponseResult::Memory(MemoryResult { memory }),
            1,
        );
        
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        
        assert_eq!(resp.id, parsed.id);
    }
    
    #[test]
    fn test_semantic_search_result_serde() {
        let result = SemanticSearchResult {
            results: vec![SearchHit {
                memory_id: Uuid::new_v4(),
                score: 0.95,
                memory_type: MemoryType::Decision,
                content_preview: "test content".to_string(),
                project_id: Some(Uuid::new_v4()),
                tags: vec!["critical".to_string()],
                is_chunked: false,
                chunk_group_id: None,
                chunk_index: None,
                chunk_total: None,
                created_at: Utc::now(),
            }],
            total: 1,
            query_embedding_time_ms: 10,
            search_time_ms: 5,
        };
        
        let json = serde_json::to_string(&result).unwrap();
        let parsed: SemanticSearchResult = serde_json::from_str(&json).unwrap();
        
        assert_eq!(result.total, parsed.total);
    }
}