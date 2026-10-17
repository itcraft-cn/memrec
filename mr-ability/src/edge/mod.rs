//! # 边生成模块
//!
//! 提供记忆图边的自动生成能力，包括：
//!
//! - [`EdgeGenerator`][]: 边生成器接口
//! - [`MockEdgeGenerator`][]: 基于规则的测试实现
//! - [`ProjectEdgeGenerator`]: 基于 project_id 的 SameProject 边生成

mod generator;

pub use generator::{EdgeGenerator, LlmEdgeGenerator, MockEdgeGenerator, ProjectEdgeGenerator};
