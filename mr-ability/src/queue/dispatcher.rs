//! # QueueDispatcher 队列调度器
//!
//! 从两个队列中按优先级取出请求，串行执行。
//!
//! ## 调度策略
//!
//! 1. 优先处理 Normal Queue
//! 2. Normal Queue 为空时处理 Dream Queue
//! 3. 双队列为空时阻塞等待

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use mr_common::types::QueueConfig;
use mr_protocol::JsonRpcResponse;

use super::types::{QueuedRequest, QueueError};

/// 请求处理器 trait。
#[async_trait::async_trait]
pub trait RequestHandler: Send + Sync {
    async fn handle(&self, request: QueuedRequest) -> JsonRpcResponse;
}

/// 队列调度器。
pub struct QueueDispatcher {
    config: QueueConfig,
    normal_queue: Arc<Mutex<VecDeque<QueuedRequest>>>,
    dream_queue: Arc<Mutex<VecDeque<QueuedRequest>>>,
    handler: Arc<dyn RequestHandler>,
}

impl QueueDispatcher {
    /// 创建新的队列调度器。
    pub fn new(config: QueueConfig, handler: Arc<dyn RequestHandler>) -> Self {
        Self {
            config,
            normal_queue: Arc::new(Mutex::new(VecDeque::new())),
            dream_queue: Arc::new(Mutex::new(VecDeque::new())),
            handler,
        }
    }

    /// 推送 Normal 请求到队列。
    pub async fn push_normal(&self, request: QueuedRequest) -> Result<(), QueueError> {
        let mut queue = self.normal_queue.lock().await;

        if queue.len() >= self.config.normal_queue_capacity {
            return Err(QueueError::Full);
        }

        queue.push_back(request);
        Ok(())
    }

    /// 推送 Dream 请求到队列。
    pub async fn push_dream(&self, request: QueuedRequest) -> Result<(), QueueError> {
        let mut queue = self.dream_queue.lock().await;

        if queue.len() >= self.config.dream_queue_capacity {
            return Err(QueueError::Full);
        }

        queue.push_back(request);
        Ok(())
    }

    /// 从队列中取出请求（优先 Normal）。
    pub async fn pop(&self) -> Option<QueuedRequest> {
        let mut normal = self.normal_queue.lock().await;

        if !normal.is_empty() {
            return normal.pop_front();
        }

        drop(normal);

        let mut dream = self.dream_queue.lock().await;

        if !dream.is_empty() {
            return dream.pop_front();
        }

        None
    }

    /// 获取队列状态。
    pub async fn status(&self) -> QueueStatus {
        let normal = self.normal_queue.lock().await;
        let dream = self.dream_queue.lock().await;

        QueueStatus {
            normal_queue_len: normal.len(),
            dream_queue_len: dream.len(),
            normal_queue_capacity: self.config.normal_queue_capacity,
            dream_queue_capacity: self.config.dream_queue_capacity,
        }
    }

    /// 运行调度循环。
    pub async fn run(self) {
        info!(
            "QueueDispatcher started: normal_capacity={}, dream_capacity={}",
            self.config.normal_queue_capacity, self.config.dream_queue_capacity
        );

        loop {
            if let Some(request) = self.pop().await {
                let is_normal = matches!(request, QueuedRequest::Normal(_));
                let _response = self.handler.handle(request).await;

                if is_normal {
                    tracing::debug!("Handled normal request");
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }
}

/// 队列状态。
#[derive(Debug, Clone)]
pub struct QueueStatus {
    pub normal_queue_len: usize,
    pub dream_queue_len: usize,
    pub normal_queue_capacity: usize,
    pub dream_queue_capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_protocol::{RequestAction, ResponseResult, SuccessResult};

    struct MockHandler;

    #[async_trait::async_trait]
    impl RequestHandler for MockHandler {
        async fn handle(&self, _request: QueuedRequest) -> JsonRpcResponse {
            JsonRpcResponse::success(
                ResponseResult::Success(SuccessResult {
                    message: "ok".to_string(),
                }),
                1,
            )
        }
    }

    #[tokio::test]
    async fn test_push_pop_normal() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let request = mr_protocol::JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
        let queued = QueuedRequest::Normal(request);

        dispatcher.push_normal(queued.clone()).await.unwrap();

        let popped = dispatcher.pop().await;
        assert!(popped.is_some());

        if let QueuedRequest::Normal(req) = popped.unwrap() {
            assert_eq!(req.id, 1);
        } else {
            panic!("Expected Normal request");
        }
    }

    #[tokio::test]
    async fn test_push_pop_dream() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let queued = QueuedRequest::Dream;

        dispatcher.push_dream(queued.clone()).await.unwrap();

        let popped = dispatcher.pop().await;
        assert!(popped.is_some());
    }

    #[tokio::test]
    async fn test_priority_normal_first() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        dispatcher.push_dream(QueuedRequest::Dream).await.unwrap();

        let normal_request = mr_protocol::JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
        dispatcher
            .push_normal(QueuedRequest::Normal(normal_request))
            .await
            .unwrap();

        let popped = dispatcher.pop().await.unwrap();
        assert!(matches!(popped, QueuedRequest::Normal(_)));
    }

    #[tokio::test]
    async fn test_queue_full() {
        let config = QueueConfig {
            normal_queue_capacity: 2,
            dream_queue_capacity: 1,
        };
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let request = mr_protocol::JsonRpcRequest::new(RequestAction::GetVersion, None, 1);

        dispatcher
            .push_normal(QueuedRequest::Normal(request.clone()))
            .await
            .unwrap();
        dispatcher
            .push_normal(QueuedRequest::Normal(request.clone()))
            .await
            .unwrap();

        let result = dispatcher.push_normal(QueuedRequest::Normal(request)).await;
        assert!(matches!(result, Err(QueueError::Full)));
    }

    #[tokio::test]
    async fn test_queue_status() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let status = dispatcher.status().await;
        assert_eq!(status.normal_queue_len, 0);
        assert_eq!(status.dream_queue_len, 0);

        dispatcher.push_normal(QueuedRequest::Dream).await.unwrap();

        let status = dispatcher.status().await;
        assert_eq!(status.normal_queue_len, 1);
    }
}