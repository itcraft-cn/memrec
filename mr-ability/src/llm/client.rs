//! # LLM 客户端
//!
//! 提供 OpenAI 兼容协议的聊天补全客户端抽象与实现。
//!
//! ## 架构
//!
//! - [`LlmClient`]：统一客户端 trait，业务代码只依赖此抽象，便于测试替换
//! - [`OpenAiCompatibleClient`]：基于 reqwest 的真实实现，支持任意 OpenAI 兼容端点
//! - [`MockLlmClient`]：测试桩，返回预设响应或固定文本
//!
//! ## 错误处理
//!
//! - 网络/协议错误统一包装为 [`LlmError`]
//! - 响应缺少可提取文本时返回 [`LlmError::EmptyResponse`]

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

use mr_common::{LlmConfig, ThinkLevel};

use super::types::{ChatCompletionRequest, ChatCompletionResponse, LlmMessage};

/// LLM 客户端错误。
#[derive(Debug, Error)]
pub enum LlmError {
    /// HTTP 请求失败
    #[error("LLM HTTP request failed: {0}")]
    Http(String),
    /// 服务返回非成功状态码
    #[error("LLM HTTP status {status}: {body}")]
    HttpStatus { status: u16, body: String },
    /// 响应解析失败
    #[error("LLM response parse failed: {0}")]
    Parse(String),
    /// 响应中没有可提取的文本
    #[error("LLM response is empty")]
    EmptyResponse,
    /// 调用方取消
    #[error("LLM request cancelled")]
    Cancelled,
}

/// LLM 客户端 trait。
///
/// 业务组件（Dream 摘要、记忆维护决策）通过此抽象调用大模型，
/// 便于注入 Mock 进行单元测试。
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发起单轮对话补全，返回最终文本。
    ///
    /// # 错误
    ///
    /// 网络失败、非 2xx 状态码、响应空文本均返回错误。
    async fn chat(&self, messages: &[LlmMessage]) -> Result<String, LlmError>;

    /// 以指定思考等级发起对话补全。
    ///
    /// 默认实现忽略覆盖值直接调用 [`chat`](Self::chat)（使用配置的 think_level），
    /// 需要按调用控制思考强度的实现（如 [`OpenAiCompatibleClient`]）覆写。
    ///
    /// 模板化/结构化生成任务（如 Dream 摘要）应传 [`ThinkLevel::Off`]
    /// 强制直白输出，避免推理内容污染结果。
    async fn chat_with_think(
        &self,
        messages: &[LlmMessage],
        think_level: mr_common::ThinkLevel,
    ) -> Result<String, LlmError> {
        let _ = think_level;
        self.chat(messages).await
    }

    /// 客户端是否已配置可用（有 API Key）。
    fn configured(&self) -> bool;
}

/// OpenAI 兼容客户端。
pub struct OpenAiCompatibleClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl OpenAiCompatibleClient {
    /// 创建客户端。
    ///
    /// `timeout_secs` 控制单次请求超时；HTTP 客户端复用连接池。
    pub fn new(config: LlmConfig) -> Self {
        let timeout = Duration::from_secs(config.timeout_secs.max(1));
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build HTTP client");
        Self { config, http }
    }

    /// 构造 chat/completions 请求 URL：`{url}/chat/completions`。
    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.config.url.trim_end_matches('/'))
    }

    /// 构造请求体，注入 provider 特有的思考参数。
    fn build_request(
        &self,
        messages: &[LlmMessage],
        think_level: ThinkLevel,
    ) -> ChatCompletionRequest {
        let mut extra = serde_json::Map::new();
        if let Some(serde_json::Value::Object(map)) =
            think_level.to_provider_param(&self.config.provider)
        {
            extra.extend(map);
        }
        ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: messages.to_vec(),
            max_tokens: Some(self.config.max_tokens),
            temperature: Some(self.config.temperature),
            extra: serde_json::Value::Object(extra),
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn chat(&self, messages: &[LlmMessage]) -> Result<String, LlmError> {
        self.chat_with_think(messages, self.config.think_level())
            .await
    }

    async fn chat_with_think(
        &self,
        messages: &[LlmMessage],
        think_level: ThinkLevel,
    ) -> Result<String, LlmError> {
        if !self.configured() {
            return Err(LlmError::Http(
                "LLM api_key is empty, check [llm] config".to_string(),
            ));
        }

        let request = self.build_request(messages, think_level);
        let request_id = Uuid::new_v4();
        debug!(
            request_id = %request_id,
            provider = %self.config.provider,
            model = %self.config.model,
            "LLM chat request"
        );

        let response = self
            .http
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        if !status.is_success() {
            return Err(LlmError::HttpStatus {
                status: status.as_u16(),
                body: body.chars().take(500).collect(),
            });
        }

        let parsed: ChatCompletionResponse =
            serde_json::from_str(&body).map_err(|e| LlmError::Parse(e.to_string()))?;

        let text = parsed
            .choices
            .first()
            .map(|c| c.message.text())
            .unwrap_or_default();

        if let Some(usage) = parsed.usage {
            info!(
                request_id = %request_id,
                prompt_tokens = usage.prompt_tokens,
                completion_tokens = usage.completion_tokens,
                "LLM chat completed"
            );
        }

        if text.is_empty() {
            warn!(request_id = %request_id, "LLM response has no content");
            return Err(LlmError::EmptyResponse);
        }

        Ok(text)
    }

    fn configured(&self) -> bool {
        !self.config.api_key.trim().is_empty()
    }
}

/// 测试桩客户端。
///
/// 可设置固定响应文本或按消息内容返回映射文本。
#[derive(Debug, Clone)]
pub struct MockLlmClient {
    /// 固定响应文本
    response: String,
    /// 是否模拟配置可用
    configured: bool,
    /// 记录调用次数
    call_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl MockLlmClient {
    /// 创建固定响应的 Mock 客户端。
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            configured: true,
            call_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// 创建模拟未配置的客户端。
    pub fn unconfigured() -> Self {
        Self {
            response: String::new(),
            configured: false,
            call_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// 获取调用次数。
    pub fn call_count(&self) -> usize {
        self.call_count.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn chat(&self, _messages: &[LlmMessage]) -> Result<String, LlmError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !self.configured {
            return Err(LlmError::Http("not configured".to_string()));
        }
        Ok(self.response.clone())
    }

    fn configured(&self) -> bool {
        self.configured
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_endpoint_joins_url() {
        let client = OpenAiCompatibleClient::new(LlmConfig::default());
        assert_eq!(
            client.endpoint(),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_request_deepseek_thinking() {
        let mut config = LlmConfig::default();
        config.provider = "deepseek".to_string();
        config.think_level = "xhigh".to_string();
        let client = OpenAiCompatibleClient::new(config);

        let req = client.build_request(&[LlmMessage::user("hello")], ThinkLevel::Xhigh);
        assert_eq!(req.extra["thinking"]["type"], "enabled");
        assert_eq!(req.extra["reasoning_effort"], "max");
        assert_eq!(req.model, "deepseek-v4-flash");
        assert_eq!(req.max_tokens, Some(4096));
    }

    #[test]
    fn test_build_request_openai_reasoning() {
        let mut config = LlmConfig::default();
        config.provider = "openai".to_string();
        config.model = "gpt-4o-mini".to_string();
        config.think_level = "medium".to_string();
        let client = OpenAiCompatibleClient::new(config);

        let req = client.build_request(&[], ThinkLevel::Medium);
        assert_eq!(req.extra["reasoning_effort"], "medium");
    }

    #[test]
    fn test_build_request_off_disables_thinking() {
        // DeepSeek 思考模式默认打开：Off 必须显式 disabled（而非不传）
        let mut config = LlmConfig::default();
        config.think_level = "off".to_string();
        let client = OpenAiCompatibleClient::new(config);

        let req = client.build_request(&[], ThinkLevel::Off);
        assert_eq!(req.extra["thinking"]["type"], "disabled");
        assert!(req.extra.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_build_request_per_call_think_override() {
        // 配置 think_level=medium（默认），按调用覆盖为 Off：显式 disabled
        let client = OpenAiCompatibleClient::new(LlmConfig::default());
        let req = client.build_request(&[], ThinkLevel::Off);
        assert_eq!(req.extra["thinking"]["type"], "disabled");

        // 覆盖为 High：deepseek 注入 reasoning_effort=high
        let req = client.build_request(&[], ThinkLevel::High);
        assert_eq!(req.extra["thinking"]["type"], "enabled");
        assert_eq!(req.extra["reasoning_effort"], "high");
    }

    #[test]
    fn test_build_request_deepseek_medium_effort() {
        // 文档映射：medium → high
        let client = OpenAiCompatibleClient::new(LlmConfig::default());
        let req = client.build_request(&[], ThinkLevel::Medium);
        assert_eq!(req.extra["thinking"]["type"], "enabled");
        assert_eq!(req.extra["reasoning_effort"], "high");
    }

    #[test]
    fn test_configured_check() {
        let client = OpenAiCompatibleClient::new(LlmConfig::default());
        assert!(!client.configured());

        let mut config = LlmConfig::default();
        config.api_key = "sk-test".to_string();
        let client = OpenAiCompatibleClient::new(config);
        assert!(client.configured());
    }

    #[test]
    fn test_arc_client_trait_object() {
        // 验证 trait object 可用（Send + Sync）
        let client: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new("ok"));
        assert!(client.configured());
    }
}
