//! # 队列类型定义

use mr_protocol::JsonRpcRequest;
use thiserror::Error;

/// 队列请求类型。
#[derive(Debug, Clone)]
pub enum QueuedRequest {
    /// 普通用户请求
    Normal(JsonRpcRequest),
    /// Dream 整合请求
    Dream,
}

/// 队列错误类型。
#[derive(Debug, Error)]
pub enum QueueError {
    #[error("Queue is full")]
    Full,
    #[error("Queue is empty")]
    Empty,
    #[error("Failed to send request: {0}")]
    Send(String),
    #[error("Failed to receive request: {0}")]
    Receive(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_protocol::RequestAction;

    #[test]
    fn test_queued_request_debug() {
        let request = JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
        let queued = QueuedRequest::Normal(request);

        let debug_str = format!("{:?}", queued);
        assert!(debug_str.contains("Normal"));
    }
}
