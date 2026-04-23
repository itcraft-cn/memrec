mod error;
mod request;
mod response;

pub use error::{MemRecError, JsonRpcError};
pub use request::{
    JsonRpcRequest, RequestAction, RequestParams,
    AddParams, GetParams, UpdateParams, DeleteParams,
    SearchParams, SearchMode, TimeRange,
    ListParams, TagParams,
    SearchMemoryParams, GetProjectInfoParams, GetVersionParams,
    ProjectCreateParams, ProjectSwitchParams, ProjectDeleteParams,
    ConfigSetParams,
};
pub use response::{
    JsonRpcResponse, ResponseResult,
    MemoryResult, MemoryListResult, SearchResult,
    SemanticSearchResult, SearchHit,
    ProjectResult, ProjectListResult, ProjectInfoResult, VersionResult,
    ConfigResult, StatsResult, SuccessResult,
};