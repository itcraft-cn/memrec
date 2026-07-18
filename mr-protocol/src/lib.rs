//! # mr-protocol JSON-RPC 协议类型库
//!
//! 本 crate 为 memrec 生态提供 JSON-RPC 2.0 通信协议类型。

pub mod error;
pub mod request;
pub mod response;

pub use error::{JsonRpcError, MemRecError};
pub use request::{
    AddParams, DeleteParams, DreamParams, GetParams, GetProjectInfoParams, JsonRpcRequest,
    ListParams, RequestAction, RequestParams, SearchMemoryParams, TieredSearchParams,
    default_hybrid_alpha, default_mmr_enabled, default_mmr_lambda,
};
pub use response::{
    DreamResult, JsonRpcResponse, MemoryListResult, MemoryResult, ProjectInfoResult,
    ResponseResult, SearchHit, SemanticSearchResult, StatsResult, SuccessResult, VersionResult,
};
