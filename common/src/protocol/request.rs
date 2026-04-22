use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::MemoryType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestAction {
    Add,
    Get,
    Update,
    Delete,
    Search,
    List,
    Tag,
    
    ProjectCreate,
    ProjectList,
    ProjectSwitch,
    ProjectDelete,
    
    ConfigGet,
    ConfigSet,
    
    Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: RequestAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<RequestParams>,
    pub id: u64,
}

impl JsonRpcRequest {
    pub fn new(method: RequestAction, params: Option<RequestParams>, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
            id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestParams {
    Add(AddParams),
    Get(GetParams),
    Update(UpdateParams),
    Delete(DeleteParams),
    Search(SearchParams),
    List(ListParams),
    Tag(TagParams),
    
    ProjectCreate(ProjectCreateParams),
    ProjectSwitch(ProjectSwitchParams),
    ProjectDelete(ProjectDeleteParams),
    
    ConfigSet(ConfigSetParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddParams {
    pub content: String,
    #[serde(default)]
    pub memory_type: MemoryType,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParams {
    pub id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateParams {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteParams {
    pub id: Uuid,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub mode: SearchMode,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<TimeRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default)]
    pub min_importance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Exact,
    Semantic,
    Hybrid,
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

fn default_top_k() -> usize { 10 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParams {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 20 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagParams {
    pub id: Uuid,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCreateParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSwitchParams {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDeleteParams {
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSetParams {
    pub key: String,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_request_creation() {
        let req = JsonRpcRequest::new(
            RequestAction::Add,
            Some(RequestParams::Add(AddParams {
                content: "test".to_string(),
                memory_type: MemoryType::Knowledge,
                tags: vec!["tag1".to_string()],
                project_id: None,
            })),
            1,
        );
        
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, 1);
    }
    
    #[test]
    fn test_request_serde() {
        let req = JsonRpcRequest::new(
            RequestAction::Get,
            Some(RequestParams::Get(GetParams {
                id: Uuid::nil(),
            })),
            1,
        );
        
        let json = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        
        assert_eq!(req.jsonrpc, parsed.jsonrpc);
    }
    
    #[test]
    fn test_search_params_defaults() {
        let params = SearchParams {
            text: None,
            mode: SearchMode::default(),
            tags: Vec::new(),
            time_range: None,
            project_id: None,
            top_k: default_top_k(),
            min_importance: 0.0,
        };
        
        assert_eq!(params.mode, SearchMode::Hybrid);
        assert_eq!(params.top_k, 10);
    }
}