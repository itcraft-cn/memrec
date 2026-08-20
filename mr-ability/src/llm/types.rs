//! # LLM 对话类型
//!
//! OpenAI 兼容 `chat/completions` 协议的最小类型集，足够覆盖主流兼容服务
//! （DeepSeek、OpenAI、通义千问、Kimi、Ollama 等）。

use serde::{Deserialize, Serialize};

/// 对话消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

/// 对话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
}

impl LlmMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::Assistant,
            content: content.into(),
        }
    }
}

/// 对话补全请求体。
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// provider 特有的思考参数（DeepSeek `thinking` / OpenAI `reasoning_effort`）
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// 对话补全响应体。
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// 响应选择项。
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
}

/// 响应消息。
///
/// 兼容 DeepSeek 推理模型：`reasoning_content` 为思考过程，`content` 为最终回答。
#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    /// DeepSeek 推理模型的思考过程字段
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

impl ResponseMessage {
    /// 提取最终文本：优先 `content`，空时回退 `reasoning_content`。
    pub fn text(&self) -> String {
        self.content
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or(self.reasoning_content.as_deref())
            .unwrap_or_default()
            .trim()
            .to_string()
    }
}

/// Token 用量。
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_message_text_prefers_content() {
        let msg = ResponseMessage {
            role: "assistant".to_string(),
            content: Some("final answer".to_string()),
            reasoning_content: Some("thinking...".to_string()),
        };
        assert_eq!(msg.text(), "final answer");
    }

    #[test]
    fn test_response_message_text_falls_back_to_reasoning() {
        let msg = ResponseMessage {
            role: "assistant".to_string(),
            content: Some("   ".to_string()),
            reasoning_content: Some("only reasoning".to_string()),
        };
        assert_eq!(msg.text(), "only reasoning");
    }

    #[test]
    fn test_response_message_text_empty() {
        let msg = ResponseMessage {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
        };
        assert_eq!(msg.text(), "");
    }

    #[test]
    fn test_chat_completion_response_parse_deepseek() {
        // DeepSeek 推理模型响应：content 可能为空，reasoning_content 有值
        let json = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "deep thinking"
                }
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }"#;
        let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices[0].message.text(), "deep thinking");
        assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 15);
    }

    #[test]
    fn test_chat_completion_request_serialize() {
        let req = ChatCompletionRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![LlmMessage::user("hi")],
            max_tokens: Some(100),
            temperature: Some(0.3),
            extra: serde_json::json!({"thinking": {"type": "enabled", "effort": "high"}}),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "deepseek-chat");
        assert_eq!(json["thinking"]["effort"], "high");
        assert_eq!(json["messages"][0]["role"], "user");
    }
}
