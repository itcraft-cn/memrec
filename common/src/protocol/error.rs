use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemRecError {
    #[error("Memory not found: {0}")]
    MemoryNotFound(uuid::Uuid),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Embedding error: {0}")]
    EmbeddingError(String),

    #[error("Memory already deleted: {0}")]
    AlreadyDeleted(uuid::Uuid),

    #[error("Recovery period expired")]
    RecoveryExpired,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
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
