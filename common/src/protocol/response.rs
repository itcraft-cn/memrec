use serde::{Deserialize, Serialize};

use crate::types::{Memory, Project};
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
    Project(ProjectResult),
    ProjectList(ProjectListResult),
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
pub struct ProjectResult {
    pub project: Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectListResult {
    pub projects: Vec<Project>,
    pub total: usize,
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
}