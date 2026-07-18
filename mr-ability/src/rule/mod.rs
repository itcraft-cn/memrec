//! # 检索规则引擎
//!
//! 实现 Curate-then-Synthesize 流程中的规则匹配与执行。
//!
//! ## 架构
//!
//! - [`RuleEngine`][]: 规则引擎，匹配和执行规则
//! - [`RuleMatcher`][]: 规则匹配器，判断查询是否命中规则
//!
//! ## 流程
//!
//! 1. 基础向量检索获取候选
//! 2. 遍历启用的规则，匹配查询条件
//! 3. 对候选执行 Boost/Suppress/Filter/Expand 动作
//! 4. 重排序后返回

mod engine;
mod matcher;

pub use engine::RuleEngine;
pub use matcher::RuleMatcher;
