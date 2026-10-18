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

use mr_common::MemoryType;

/// JSON-RPC 请求方法枚举。
///
/// 序列化为 `snake_case`，作为 [`JsonRpcRequest`] 的 `method` 字段。
/// 守护进程 Router 据此分发到对应的处理函数。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RequestAction {
    Add,
    Get,
    Update,
    Delete,
    Restore,
    Search,
    List,
    Tag,

    SearchMemory,
    TieredSearch,
    GetProjectInfo,
    GetVersion,
    Dream,
    DreamReport,

    ProjectCreate,
    ProjectList,
    ProjectSwitch,
    ProjectDelete,

    ConfigGet,
    ConfigSet,

    Stats,

    SceneCreate,
    SceneGet,
    SceneList,
    SceneDelete,
    SceneAddMemory,
    SceneRemoveMemory,
    SceneUpdateHeat,
}

impl std::fmt::Display for RequestAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestAction::Add => write!(f, "add"),
            RequestAction::Get => write!(f, "get"),
            RequestAction::Update => write!(f, "update"),
            RequestAction::Delete => write!(f, "delete"),
            RequestAction::Restore => write!(f, "restore"),
            RequestAction::Search => write!(f, "search"),
            RequestAction::List => write!(f, "list"),
            RequestAction::Tag => write!(f, "tag"),
            RequestAction::SearchMemory => write!(f, "search_memory"),
            RequestAction::TieredSearch => write!(f, "tiered_search"),
            RequestAction::GetProjectInfo => write!(f, "get_project_info"),
            RequestAction::GetVersion => write!(f, "get_version"),
            RequestAction::Dream => write!(f, "dream"),
            RequestAction::DreamReport => write!(f, "dream_report"),
            RequestAction::ProjectCreate => write!(f, "project_create"),
            RequestAction::ProjectList => write!(f, "project_list"),
            RequestAction::ProjectSwitch => write!(f, "project_switch"),
            RequestAction::ProjectDelete => write!(f, "project_delete"),
            RequestAction::ConfigGet => write!(f, "config_get"),
            RequestAction::ConfigSet => write!(f, "config_set"),
            RequestAction::Stats => write!(f, "stats"),
            RequestAction::SceneCreate => write!(f, "scene_create"),
            RequestAction::SceneGet => write!(f, "scene_get"),
            RequestAction::SceneList => write!(f, "scene_list"),
            RequestAction::SceneDelete => write!(f, "scene_delete"),
            RequestAction::SceneAddMemory => write!(f, "scene_add_memory"),
            RequestAction::SceneRemoveMemory => write!(f, "scene_remove_memory"),
            RequestAction::SceneUpdateHeat => write!(f, "scene_update_heat"),
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
    Restore(RestoreParams),
    Search(SearchParams),
    List(ListParams),
    Tag(TagParams),

    SearchMemory(SearchMemoryParams),
    TieredSearch(TieredSearchParams),
    GetProjectInfo(GetProjectInfoParams),
    GetVersion(GetVersionParams),
    Dream(DreamParams),
    DreamReport(DreamReportParams),

    ProjectCreate(ProjectCreateParams),
    ProjectList(ProjectListParams),
    ProjectSwitch(ProjectSwitchParams),
    ProjectDelete(ProjectDeleteParams),

    ConfigGet(ConfigGetParams),
    ConfigSet(ConfigSetParams),

    SceneCreate(SceneCreateParams),
    SceneGet(SceneGetParams),
    SceneList(SceneListParams),
    SceneDelete(SceneDeleteParams),
    SceneAddMemory(SceneAddMemoryParams),
    SceneRemoveMemory(SceneRemoveMemoryParams),
    SceneUpdateHeat(SceneUpdateHeatParams),
}

pub use dream::*;
pub use memory::*;
pub use project::*;
pub use scene::*;

mod dream;
mod memory;
mod project;
mod scene;

#[cfg(test)]
mod tests;
