use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::MemoryType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RequestAction {
    Add,
    Get,
    Update,
    Delete,
    Search,
    List,
    Tag,
    
    SearchMemory,
    GetProjectInfo,
    GetVersion,
    
    ProjectCreate,
    ProjectList,
    ProjectSwitch,
    ProjectDelete,
    
    ConfigGet,
    ConfigSet,
    
    Stats,
}

impl std::fmt::Display for RequestAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestAction::Add => write!(f, "add"),
            RequestAction::Get => write!(f, "get"),
            RequestAction::Update => write!(f, "update"),
            RequestAction::Delete => write!(f, "delete"),
            RequestAction::Search => write!(f, "search"),
            RequestAction::List => write!(f, "list"),
            RequestAction::Tag => write!(f, "tag"),
            RequestAction::SearchMemory => write!(f, "search_memory"),
            RequestAction::GetProjectInfo => write!(f, "get_project_info"),
            RequestAction::GetVersion => write!(f, "get_version"),
            RequestAction::ProjectCreate => write!(f, "project_create"),
            RequestAction::ProjectList => write!(f, "project_list"),
            RequestAction::ProjectSwitch => write!(f, "project_switch"),
            RequestAction::ProjectDelete => write!(f, "project_delete"),
            RequestAction::ConfigGet => write!(f, "config_get"),
            RequestAction::ConfigSet => write!(f, "config_set"),
            RequestAction::Stats => write!(f, "stats"),
        }
    }
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
    
    SearchMemory(SearchMemoryParams),
    GetProjectInfo(GetProjectInfoParams),
    GetVersion(GetVersionParams),
    
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
    #[serde(default)]
    pub is_global: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParams {
    pub id: Uuid,
    #[serde(default)]
    pub merge: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryParams {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default = "default_include_global")]
    pub include_global: bool,
    #[serde(default)]
    pub project_only: bool,
    #[serde(default)]
    pub global_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_min_score")]
    pub min_score: f32,
}

pub fn default_include_global() -> bool { true }
pub fn default_min_score() -> f32 {
    std::env::var("MEMREC_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.75)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetProjectInfoParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetVersionParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SearchMode {
    Exact,
    Semantic,
    #[default]
    Hybrid,
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
    #[serde(default)]
    pub project_only: bool,
    #[serde(default)]
    pub global_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
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
                is_global: false,
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
                merge: false,
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
    
    #[test]
    fn test_search_memory_params_defaults() {
        let params = SearchMemoryParams {
            query: "test".to_string(),
            project_id: None,
            include_global: default_include_global(),
            project_only: false,
            global_only: false,
            memory_type: None,
            top_k: default_top_k(),
            min_score: default_min_score(),
        };
        
        assert_eq!(params.include_global, true);
        assert_eq!(params.top_k, 10);
        assert_eq!(params.min_score, 0.75);
    }
    
    #[test]
    fn test_search_memory_params_serde() {
        let params = SearchMemoryParams {
            query: "authentication".to_string(),
            project_id: Some(Uuid::new_v4()),
            include_global: true,
            project_only: false,
            global_only: false,
            memory_type: Some(MemoryType::Decision),
            top_k: 20,
            min_score: 0.8,
        };
        
        let json = serde_json::to_string(&params).unwrap();
        let parsed: SearchMemoryParams = serde_json::from_str(&json).unwrap();
        
        assert_eq!(params.query, parsed.query);
        assert_eq!(params.top_k, parsed.top_k);
    }
}