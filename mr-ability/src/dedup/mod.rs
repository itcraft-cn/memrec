//! # 写入去重决策
//!
//! 借鉴 Mem0 的 ADD/UPDATE/DELETE/NOOP 四操作理念，在记忆写入前进行相似性
//! 检测并决策处理方式，防止记忆库无限膨胀和同义重复。
//!
//! ## 决策策略
//!
//! - [`RuleDedupDecider`]：写入时的同步规则决策（快路径，无需 LLM）。基于相似度阈值：
//!   - `score >= duplicate_threshold` → [`DedupAction::Skip`]（重复，跳过写入）
//!   - `update_threshold <= score < duplicate_threshold` → [`DedupAction::Update`]（合并更新）
//!   - 低于 `update_threshold` → [`DedupAction::Add`]（正常新增）
//!
//! 语义级冲突（重复/矛盾/过时）由 [`review`] 模块在写入后异步审查（慢路径），
//! 结果只做标记失效或软删新记录，不改写正文。

pub mod decider;
pub mod review;

pub use decider::{DedupAction, DedupDecider, DedupDecision, RuleDedupDecider};
pub use review::{
    mark_superseded, parse_review_response, ConflictReviewer, LlmConflictReviewer, ReviewAction,
    ReviewDecision,
};
