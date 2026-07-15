//! # JSON-RPC 2.0 响应定义
//!
//! 定义守护进程返回给客户端的所有响应类型，包括：
//!
//! - [`JsonRpcResponse`]：JSON-RPC 2.0 标准响应包装，成功时携带 [`ResponseResult`]，失败时携带 [`JsonRpcError`]
//! - [`ResponseResult`]：响应结果联合类型，使用内部标签区分各操作返回值
//! - 各操作的具体返回结构：[`MemoryResult`]、[`SemanticSearchResult`]、[`StatsResult`] 等
//!
//! ## 设计要点
//!
//! - [`SearchHit`] 包含记忆摘要预览（`content_preview`）和分块信息，避免大内容直接传输
//! - [`SemanticSearchResult`] 分别记录嵌入生成耗时与搜索耗时，便于性能分析
//! - [`SuccessResult`] 实现了 `From<bool>` 便于从布尔值构造成功/失败消息

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::JsonRpcError;
use crate::types::{Memory, MemoryType, Project};

/// JSON-RPC 2.0 标准响应对象。
///
/// 成功时 `result` 为 `Some`、`error` 为 `None`；失败时反之。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC 协议版本，固定为 `"2.0"`
    pub jsonrpc: String,
    /// 成功时的返回结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ResponseResult>,
    /// 失败时的错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// 请求 ID，与请求中的 `id` 对应
    pub id: u64,
}

impl JsonRpcResponse {
    /// 构造成功响应。
    pub fn success(result: ResponseResult, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// 构造错误响应。
    pub fn error(err: JsonRpcError, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(err),
            id,
        }
    }
}

/// 响应结果联合类型。
///
/// 使用 `#[serde(tag = "type")]` 内部标签序列化，客户端可根据 `type` 字段区分结果类型。
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
    Version(VersionResult),
    Config(ConfigResult),
    Stats(StatsResult),
    Success(SuccessResult),
}

/// 单条记忆操作结果，用于 Add/Get/Update 等操作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub memory: Memory,
}

/// 记忆列表结果，用于 List 操作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListResult {
    /// 匹配的记忆列表
    pub memories: Vec<Memory>,
    /// 总匹配数（可能大于返回数量）
    pub total: usize,
}

/// 精确搜索结果，用于 Search 操作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memories: Vec<Memory>,
    pub total: usize,
    /// 搜索耗时（毫秒）
    pub elapsed_ms: u64,
}

/// 语义搜索结果，用于 SearchMemory 操作。
///
/// 结果以 [`SearchHit`] 摘要形式返回，不含完整内容，减少传输开销。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    /// 匹配的记忆摘要列表，按相似度降序排列
    pub results: Vec<SearchHit>,
    pub total: usize,
    /// 嵌入向量生成耗时（毫秒）
    pub query_embedding_time_ms: u64,
    /// 向量搜索耗时（毫秒）
    pub search_time_ms: u64,
}

/// 语义搜索命中摘要。
///
/// 包含记忆的元信息和内容预览，不含完整 `content` 和 `embedding`，
/// 客户端可通过 `memory_id` 调用 Get 获取完整内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub memory_id: Uuid,
    /// 与查询的余弦相似度分数
    pub score: f32,
    pub memory_type: MemoryType,
    /// 内容前若干字符的预览
    pub content_preview: String,
    pub project_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub is_chunked: bool,
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
    pub created_at: DateTime<Utc>,
}

/// 单个项目操作结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResult {
    pub project: Project,
}

/// 项目列表结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectListResult {
    pub projects: Vec<Project>,
    pub total: usize,
}

/// 当前项目信息结果，由 GetProjectInfo 操作返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfoResult {
    pub project_id: Uuid,
    pub project_name: Option<String>,
    /// 项目根目录路径
    pub project_root: String,
    /// 该项目下的记忆数量
    pub memory_count: usize,
    /// `.mr_pid` 文件是否存在
    pub mr_pid_exists: bool,
}

/// 版本查询结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResult {
    pub version: String,
}

/// 配置查询/设置结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResult {
    pub key: String,
    pub value: String,
}

/// 统计信息结果，由 Stats 操作返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub total_memories: usize,
    pub active_memories: usize,
    pub deleted_memories: usize,
    /// 存储使用率（0.0~1.0）
    pub storage_usage: f32,
    /// 全部活跃记忆的平均重要性
    pub avg_importance: f32,
}

/// 通用成功/失败结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResult {
    pub message: String,
}

/// 从布尔值构造成功/失败消息。
///
/// `true` 映射为 `"Success"`，`false` 映射为 `"Failed"`。
impl From<bool> for SuccessResult {
    fn from(success: bool) -> Self {
        Self {
            message: if success {
                "Success".to_string()
            } else {
                "Failed".to_string()
            },
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
        let resp = JsonRpcResponse::success(ResponseResult::Memory(MemoryResult { memory }), 1);

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
        let resp = JsonRpcResponse::success(ResponseResult::Memory(MemoryResult { memory }), 1);

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
