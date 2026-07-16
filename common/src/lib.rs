//! # memrec-common 共享库
//!
//! 本 crate 为 memrecd 守护进程、memrec CLI 客户端、mr-install 安装器提供共享的类型定义与通信协议。
//!
//! ## 模块组织
//!
//! - [`protocol`]：JSON-RPC 2.0 通信协议，包含请求/响应/错误类型
//! - [`types`]：核心数据模型，包含记忆、项目、配置、嵌入模型等类型定义
//!
//! ## 顶层重导出
//!
//! 为方便下游 crate 使用，本库将常用类型在顶层重导出，可直接 `use memrec_common::Memory` 使用。

pub mod protocol;
pub mod types;

pub use types::{ImportanceConfig, MemoryConfig, ServerConfig};
pub use types::{Memory, MemoryScope, MemorySource, MemoryType, Project, ProjectConfig};
pub use types::{ModelConfig, ModelFile, ModelFileType, ModelType, PoolingStrategy};

pub use protocol::ProjectInfoResult;
pub use protocol::SearchMemoryParams;
pub use protocol::{
    default_hybrid_alpha, default_include_global, default_min_score, default_mmr_enabled,
    default_mmr_lambda,
};
pub use protocol::{AddParams, GetParams, ListParams};
pub use protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, MemRecError};
pub use protocol::{MemoryListResult, MemoryResult, StatsResult, SuccessResult};
pub use protocol::{RequestAction, RequestParams, ResponseResult};
pub use protocol::{SearchHit, SemanticSearchResult};
