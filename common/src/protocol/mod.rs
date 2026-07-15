//! # JSON-RPC 2.0 通信协议
//!
//! 定义 memrec CLI 客户端与 memrecd 守护进程之间的通信协议。
//!
//! ## 协议层结构
//!
//! - [`error`]：错误类型，包含业务错误 [`MemRecError`] 与 JSON-RPC 标准错误 [`JsonRpcError`]
//! - [`request`]：请求定义，包含 [`JsonRpcRequest`]、[`RequestAction`] 枚举与各操作的参数结构
//! - [`response`]：响应定义，包含 [`JsonRpcResponse`]、[`ResponseResult`] 枚举与各操作的返回结构
//!
//! ## 通信模型
//!
//! 客户端通过 Unix Socket 发送 [`JsonRpcRequest`]，服务端经 [`crate::server::Router`] 路由分发后返回 [`JsonRpcResponse`]。
//! 请求方法由 [`RequestAction`] 枚举标识，参数通过 [`RequestParams`] 内部标签（internally tagged）区分类型。

mod error;
mod request;
mod response;

pub use error::{JsonRpcError, MemRecError};
pub use request::{
    default_include_global, default_min_score, AddParams, ConfigSetParams, DeleteParams, GetParams,
    GetProjectInfoParams, GetVersionParams, JsonRpcRequest, ListParams, ProjectCreateParams,
    ProjectDeleteParams, ProjectSwitchParams, RequestAction, RequestParams, SearchMemoryParams,
    SearchMode, SearchParams, TagParams, TimeRange, UpdateParams,
};
pub use response::{
    ConfigResult, JsonRpcResponse, MemoryListResult, MemoryResult, ProjectInfoResult,
    ProjectListResult, ProjectResult, ResponseResult, SearchHit, SearchResult,
    SemanticSearchResult, StatsResult, SuccessResult, VersionResult,
};
