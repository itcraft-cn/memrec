//! # 分层呈现引擎
//!
//! 实现检索结果按相关度分层呈现，控制 Token 预算。
//!
//! ## 架构
//!
//! - [`TierAssigner`]: 根据 score 分配 RepresentationTier
//! - [`BudgetController`]: 控制 Token 总预算与类型优先级，必要时降级/丢弃
//! - [`TieredEngine`][]: 分层检索引擎，协调上述组件
//!
//! ## 流程
//!
//! 1. 基础检索获取候选列表（score）
//! 2. TierAssigner 分配初始 tier
//! 3. BudgetController 按总预算与类型上限分配，必要时降级
//! 4. 仅为保留条目生成内容（Full/Truncated/Summary/DenseProxy）
//! 5. 返回 TieredMemory 列表

mod assigner;
mod budget;
mod engine;

pub use assigner::TierAssigner;
pub use budget::{truncate_to_tokens, BudgetController, BudgetItem};
pub use engine::{build_tiered_result, TieredEngine, TieredEngineConfig};
