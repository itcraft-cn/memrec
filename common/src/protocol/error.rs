//! # 协议错误类型
//!
//! 定义业务层错误与 JSON-RPC 标准错误，以及二者之间的转换关系。
//!
//! ## 错误分层
//!
//! - [`MemRecError`]：业务层错误枚举，使用 `thiserror` 派生，用于守护进程内部错误传递
//! - [`JsonRpcError`]：JSON-RPC 2.0 标准错误对象，序列化后随响应返回给客户端
//!
//! ## 错误码约定
//!
//! JSON-RPC 保留 `-32700` 至 `-32000` 段为协议错误，本项目自定义错误码从 `-32001` 起递减分配。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 业务层错误枚举。
///
/// 用于守护进程内部错误传递与 `?` 运算符传播。通过 [`From<MemRecError> for JsonRpcError`]
/// 实现可自动转换为 JSON-RPC 错误返回给客户端。
#[derive(Debug, Error)]
pub enum MemRecError {
    /// 记忆不存在，携带记忆 UUID
    #[error("Memory not found: {0}")]
    MemoryNotFound(uuid::Uuid),

    /// 项目不存在，携带项目名称
    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    /// 存储层错误，携带底层错误描述
    #[error("Storage error: {0}")]
    StorageError(String),

    /// 请求参数非法，携带具体原因
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// 通信连接错误，携带错误描述
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// 嵌入向量生成错误，携带错误描述
    #[error("Embedding error: {0}")]
    EmbeddingError(String),

    /// 记忆已被软删除，携带记忆 UUID
    #[error("Memory already deleted: {0}")]
    AlreadyDeleted(uuid::Uuid),

    /// 软删除恢复期已过，无法恢复
    #[error("Recovery period expired")]
    RecoveryExpired,
}

/// 将业务错误映射为 JSON-RPC 错误，同时分配对应的错误码。
///
/// 错误码分配如下：
/// - `-32001`：记忆不存在
/// - `-32002`：项目不存在
/// - `-32003`：存储错误
/// - `-32600`：请求非法（JSON-RPC 标准码）
/// - `-32004`：连接错误
/// - `-32005`：嵌入错误
/// - `-32006`：记忆已删除
/// - `-32007`：恢复期过期
impl From<MemRecError> for JsonRpcError {
    fn from(err: MemRecError) -> Self {
        JsonRpcError {
            code: match err {
                MemRecError::MemoryNotFound(_) => -32001,
                MemRecError::ProjectNotFound(_) => -32002,
                MemRecError::StorageError(_) => -32003,
                MemRecError::InvalidRequest(_) => -32600,
                MemRecError::ConnectionError(_) => -32004,
                MemRecError::EmbeddingError(_) => -32005,
                MemRecError::AlreadyDeleted(_) => -32006,
                MemRecError::RecoveryExpired => -32007,
            },
            message: err.to_string(),
            data: None,
        }
    }
}

/// JSON-RPC 2.0 标准错误对象。
///
/// 序列化后嵌入 [`crate::protocol::JsonRpcResponse`] 的 `error` 字段返回给客户端。
/// `data` 字段为可选的附加详情，序列化时为 `None` 则省略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// 错误码，遵循 JSON-RPC 2.0 规范
    pub code: i64,
    /// 人类可读的错误描述
    pub message: String,
    /// 可选的附加错误数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_to_jsonrpc() {
        let err = MemRecError::MemoryNotFound(uuid::Uuid::nil());
        let rpc_err: JsonRpcError = err.into();

        assert_eq!(rpc_err.code, -32001);
        assert!(rpc_err.message.contains("not found"));
    }

    #[test]
    fn test_jsonrpc_error_serde() {
        let err = JsonRpcError {
            code: -32600,
            message: "Invalid Request".to_string(),
            data: Some("details".to_string()),
        };

        let json = serde_json::to_string(&err).unwrap();
        let parsed: JsonRpcError = serde_json::from_str(&json).unwrap();

        assert_eq!(err.code, parsed.code);
        assert_eq!(err.data, parsed.data);
    }
}
