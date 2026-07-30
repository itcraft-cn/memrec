//! # 请求队列模块
//!
//! 提供双队列调度能力：Normal Queue (高优先级) 和 Dream Queue (低优先级)。
//!
//! ## 调度策略
//!
//! 1. 优先处理 Normal Queue
//! 2. Normal Queue 为空时处理 Dream Queue
//! 3. 双队列为空时阻塞等待

mod dispatcher;
mod types;

pub use dispatcher::QueueDispatcher;
pub use types::{QueuedRequest, QueueError};