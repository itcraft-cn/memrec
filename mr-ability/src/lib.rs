//! # mr-ability 核心能力库
//!
//! 提供记忆存储、语义搜索、项目隔离等核心服务。

// `async_trait` 宏会为 trait 方法生成带回返回类型的 `#[must_use]`，若返回类型本身
// 已是 must_use（如 `Result`），clippy 的 double_must_use 会对每一处展开报一次。
// 这些告警来自宏展开而非本 crate 代码，且无法在不改动宏的前提下消除，故整体豁免；
// 本 crate 自身手写的 `#[must_use]` 仍受其他 clippy 规则约束。
#![allow(clippy::double_must_use)]

pub mod archive;
pub mod dedup;
pub mod dream;
pub mod edge;
pub mod embedding;
pub mod expiry;
pub mod facet;
pub mod idle;
pub mod importance;
pub mod lifecycle;
pub mod llm;
pub mod project;
pub mod propagation;
pub mod queue;
pub mod rule;
pub mod search;
pub mod storage;
pub mod tiered;
