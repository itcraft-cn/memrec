//! # Dream 传播引擎
//!
//! 实现 Dream 整合后的增量传播，将变化传播到记忆图邻居。
//!
//! ## 架构
//!
//! - [`PropagationEngine`][]: 核心引擎，生成和应用传播增量
//! - [`MockDeltaGenerator`][]: 测试用的 Mock 增量生成器
//!
//! ## 流程
//!
//! 1. Dream 整合完成后，获取被整合记忆的邻居
//! 2. 为每个邻居生成 PropagationDelta
//! 3. 推入传播队列，异步应用

mod engine;
mod generator;

pub use engine::{PropagationEngine, PropagationResult};
pub use generator::{DeltaGenerator, MockDeltaGenerator};
