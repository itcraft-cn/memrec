//! # LLM 接入模块
//!
//! 提供 OpenAI 兼容大模型客户端，供 Dream 整合摘要、记忆维护决策等能力使用。
//!
//! ## 模块组织
//!
//! - [`client`]：客户端 trait 与实现（OpenAI 兼容 / Mock）
//! - [`types`]：对话协议类型

pub mod client;
pub mod types;

pub use client::{LlmClient, LlmError, MockLlmClient, OpenAiCompatibleClient};
pub use types::{ChatCompletionRequest, ChatCompletionResponse, LlmMessage, LlmRole};
