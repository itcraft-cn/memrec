//! # 写入去重决策
//!
//! 借鉴 Mem0 的 ADD/UPDATE/DELETE/NOOP 四操作理念，在记忆写入前进行相似性
//! 检测并决策处理方式，防止记忆库无限膨胀和同义重复。
//!
//! ## 决策策略
//!
//! - [`RuleDedupDecider`]：规则版（当前默认）。基于相似度阈值：
//!   - `score >= duplicate_threshold` → [`DedupAction::Skip`]（重复，跳过写入）
//!   - `update_threshold <= score < duplicate_threshold` → [`DedupAction::Update`]（合并更新）
//!   - 低于 `update_threshold` → [`DedupAction::Add`]（正常新增）
//! - [`LlmDedupDecider`]：LLM 演进版（预留）。后续接入 LLM 工具调用实现
//!   精确的 UPDATE/DELETE 判断（含矛盾消解），当前保持占位。

pub mod decider;

pub use decider::{DedupAction, DedupDecider, DedupDecision, LlmDedupDecider, RuleDedupDecider};
