//! # JSON-RPC 2.0 请求定义
//!
//! 定义客户端发送给守护进程的所有请求类型，包括：
//!
//! - [`RequestAction`]：请求方法枚举，作为 JSON-RPC `method` 字段
//! - [`RequestParams`]：请求参数联合类型，使用内部标签（`#[serde(tag = "type")]`）区分
//! - 各操作的具体参数结构：[`AddParams`]、[`SearchMemoryParams`]、[`ListParams`] 等
//!
//! ## 默认值策略
//!
//! - [`default_min_score`]：语义搜索最低相似度，优先读取 `MEMREC_MIN_SCORE` 环境变量，默认 0.5
//! - [`default_include_global`]：搜索是否包含公共记忆，默认 true
//! - [`default_top_k`] / [`default_limit`]：返回数量上限，分别默认 10 和 20

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::MemoryType;

/// JSON-RPC 请求方法枚举。
///
/// 序列化为 `snake_case`，作为 [`JsonRpcRequest`] 的 `method` 字段。
/// 守护进程 [`Router`](crate::server::Router) 据此分发到对应的处理函数。
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

/// JSON-RPC 2.0 请求对象。
///
/// 每个请求携带唯一的 `id`，服务端在响应中回传此 ID 以便客户端匹配。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: RequestAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<RequestParams>,
    pub id: u64,
}

impl JsonRpcRequest {
    /// 构造新的 JSON-RPC 请求，`jsonrpc` 固定为 `"2.0"`。
    pub fn new(method: RequestAction, params: Option<RequestParams>, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
            id,
        }
    }
}

/// 请求参数联合类型。
///
/// 使用 `#[serde(tag = "type")]` 内部标签序列化，反序列化时根据 `type` 字段
/// 自动路由到对应的参数结构。
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

/// 添加记忆的参数。
///
/// `working_dir` 用于服务端自动检测项目 ID（通过 `.mr_pid` 文件）。
/// `is_global` 为 true 时，该记忆对所有项目可见。
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// 获取单条记忆的参数。
///
/// `merge` 为 true 时，若该记忆属于分块组，则合并所有分块内容返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParams {
    pub id: Uuid,
    #[serde(default)]
    pub merge: bool,
}

/// 更新记忆的参数。
///
/// 仅更新提供的字段，未提供的字段保持不变。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateParams {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// 删除记忆的参数。
///
/// `force` 为 true 时跳过软删除恢复期，直接永久删除。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteParams {
    pub id: Uuid,
    #[serde(default)]
    pub force: bool,
}

/// 精确搜索参数（按标签/时间范围/重要性过滤）。
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

/// 语义搜索参数，通过嵌入向量相似度检索记忆。
///
/// 搜索范围由 `include_global`/`project_only`/`global_only`/`cross_project` 四个标志控制，
/// 互斥关系由服务端处理。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryParams {
    /// 搜索查询文本，将被转换为嵌入向量
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    /// 是否包含公共记忆（默认 true）
    #[serde(default = "default_include_global")]
    pub include_global: bool,
    /// 仅搜索当前项目记忆
    #[serde(default)]
    pub project_only: bool,
    /// 仅搜索公共记忆
    #[serde(default)]
    pub global_only: bool,
    /// 跨所有项目搜索
    #[serde(default)]
    pub cross_project: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// 最低相似度阈值，低于此值的命中将被过滤
    #[serde(default = "default_min_score")]
    pub min_score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,
    #[serde(default = "default_mmr_enabled")]
    pub mmr_enabled: bool,
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,
}

/// 搜索时是否包含公共记忆的默认值。
pub fn default_include_global() -> bool {
    true
}

pub fn default_hybrid_alpha() -> f64 {
    0.5
}

pub fn default_mmr_enabled() -> bool {
    true
}

pub fn default_mmr_lambda() -> f64 {
    0.5
}

/// 语义搜索最低相似度的默认值。
///
/// 优先读取 `MEMREC_MIN_SCORE` 环境变量，解析失败时回退到 0.5。
/// 注意：实际运行时由 `ModelType::default_min_score()` 覆盖（MiniLM=0.75, BGE-M3=0.5）。
pub fn default_min_score() -> f32 {
    std::env::var("MEMREC_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.5)
}

/// 获取当前项目信息的参数。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetProjectInfoParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

/// 获取服务版本的参数（当前无字段，保留扩展性）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetVersionParams;

/// 搜索模式：精确匹配 / 语义检索 / 混合（默认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SearchMode {
    Exact,
    Semantic,
    #[default]
    Hybrid,
}

/// 时间范围过滤条件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// 搜索结果返回数量的默认值。
fn default_top_k() -> usize {
    10
}

/// 列出记忆的参数。
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

/// 列出记忆的默认数量限制。
fn default_limit() -> usize {
    20
}

/// 为记忆添加标签的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagParams {
    pub id: Uuid,
    pub tag: String,
}

/// 创建项目的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCreateParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 切换当前活跃项目的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSwitchParams {
    pub name: String,
}

/// 删除项目的参数。
///
/// `force` 为 true 时跳过确认直接删除。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDeleteParams {
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

/// 设置配置项的参数。
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
                working_dir: None,
                source: None,
                scope: None,
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
            cross_project: false,
            memory_type: None,
            top_k: default_top_k(),
            min_score: default_min_score(),
            working_dir: None,
            hybrid_alpha: default_hybrid_alpha(),
            mmr_enabled: default_mmr_enabled(),
            mmr_lambda: default_mmr_lambda(),
        };

        assert_eq!(params.include_global, true);
        assert_eq!(params.top_k, 10);
        assert_eq!(params.min_score, 0.5);
    }

    #[test]
    fn test_search_memory_params_serde() {
        let params = SearchMemoryParams {
            query: "authentication".to_string(),
            project_id: Some(Uuid::new_v4()),
            include_global: true,
            project_only: false,
            global_only: false,
            cross_project: false,
            memory_type: Some(MemoryType::Decision),
            top_k: 20,
            min_score: 0.8,
            working_dir: None,
            hybrid_alpha: default_hybrid_alpha(),
            mmr_enabled: default_mmr_enabled(),
            mmr_lambda: default_mmr_lambda(),
        };

        let json = serde_json::to_string(&params).unwrap();
        let parsed: SearchMemoryParams = serde_json::from_str(&json).unwrap();

        assert_eq!(params.query, parsed.query);
        assert_eq!(params.top_k, parsed.top_k);
    }

    #[test]
    fn test_add_params_with_source_scope() {
        let json = r#"{
            "type": "add",
            "content": "test content",
            "source": "system",
            "scope": "global"
        }"#;

        let params: RequestParams = serde_json::from_str(json).unwrap();
        if let RequestParams::Add(p) = params {
            assert_eq!(p.source, Some("system".to_string()));
            assert_eq!(p.scope, Some("global".to_string()));
        } else {
            panic!("Expected Add params");
        }
    }

    #[test]
    fn test_search_memory_params_hybrid() {
        let json = r#"{
            "type": "search_memory",
            "query": "test query",
            "hybrid_alpha": 0.7,
            "mmr_enabled": false
        }"#;

        let params: RequestParams = serde_json::from_str(json).unwrap();
        if let RequestParams::SearchMemory(p) = params {
            assert_eq!(p.hybrid_alpha, 0.7);
            assert!(!p.mmr_enabled);
        } else {
            panic!("Expected SearchMemory params");
        }
    }
}
