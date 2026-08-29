//! # LLM 接入配置
//!
//! 定义 OpenAI 兼容大模型接入配置，供 Dream 整合摘要、记忆维护决策等能力使用。
//!
//! ## 设计说明
//!
//! - 仅支持 OpenAI 兼容协议（`/chat/completions`），可对接 DeepSeek、OpenAI、
//!   通义千问、Kimi、本地 Ollama 等任意兼容服务
//! - `think_level` 控制思考强度，具体参数格式由 [`ThinkLevel::to_provider_param`] 按
//!   provider 映射（DeepSeek 使用 `thinking` 字段，OpenAI 使用 `reasoning_effort`）
//! - 未配置（`enabled = false`）时，依赖 LLM 的能力（如 Dream 摘要）视为不可用

use serde::{Deserialize, Serialize};

/// 思考强度等级。
///
/// 参照 DeepSeek 模型目录的 `reasoningEffortMap` 设计：
/// - `minimal`/`low`/`medium`/`high` 映射为 DeepSeek 的 `high`
/// - `xhigh` 映射为 DeepSeek 的 `max`
/// - `off` 表示关闭思考（DeepSeek 的 `disabled`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThinkLevel {
    /// 关闭思考（仅非推理模型或显式关闭）
    Off,
    /// 最小思考
    Minimal,
    /// 低强度思考
    Low,
    /// 中强度思考（默认）
    #[default]
    Medium,
    /// 高强度思考
    High,
    /// 极强思考（映射 DeepSeek max）
    Xhigh,
}

impl ThinkLevel {
    /// 按 provider 生成思考参数（完整顶层参数对象）。
    ///
    /// # 返回
    ///
    /// - DeepSeek 系（provider 含 `deepseek`）：`{"thinking":{"type":"enabled","effort":"high|max"}}`
    /// - OpenAI 系（provider 含 `openai`）：`{"reasoning_effort":"minimal|low|medium|high"}`
    /// - 其他 provider 或 `Off`：`None`（不传思考参数）
    pub fn to_provider_param(&self, provider: &str) -> Option<serde_json::Value> {
        let p = provider.to_lowercase();
        if p.contains("deepseek") {
            let effort = match self {
                Self::Off => return None,
                Self::Minimal | Self::Low | Self::Medium | Self::High => "high",
                Self::Xhigh => "max",
            };
            Some(serde_json::json!({
                "thinking": {
                    "type": "enabled",
                    "effort": effort,
                }
            }))
        } else if p.contains("openai") {
            match self {
                Self::Off => None,
                Self::Minimal => Some(serde_json::json!({"reasoning_effort": "minimal"})),
                Self::Low => Some(serde_json::json!({"reasoning_effort": "low"})),
                Self::Medium => Some(serde_json::json!({"reasoning_effort": "medium"})),
                Self::High | Self::Xhigh => Some(serde_json::json!({"reasoning_effort": "high"})),
            }
        } else {
            None
        }
    }
}

/// LLM 接入配置（OpenAI 兼容模式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// 是否启用 LLM。关闭时依赖 LLM 的能力（如 Dream 摘要）不可用
    #[serde(default)]
    pub enabled: bool,

    /// provider 名称，如 `deepseek` / `openai` / `ollama` / `custom`
    /// 决定思考参数的注入格式
    #[serde(default = "default_llm_provider")]
    pub provider: String,

    /// OpenAI 兼容端点地址，如 `https://api.deepseek.com/v1`
    #[serde(default = "default_llm_url", alias = "base_url")]
    pub url: String,

    /// API Key
    #[serde(default, alias = "apiKey")]
    pub api_key: String,

    /// 模型名称，如 `deepseek-chat` / `deepseek-reasoner` / `gpt-4o-mini`
    #[serde(default = "default_llm_model")]
    pub model: String,

    /// 思考强度：`off` / `minimal` / `low` / `medium` / `high` / `xhigh`
    #[serde(default = "default_think_level_str", alias = "thinkLevel")]
    pub think_level: String,

    /// 请求超时（秒）
    #[serde(default = "default_llm_timeout")]
    pub timeout_secs: u64,

    /// 生成的最大 token 数
    #[serde(default = "default_llm_max_tokens")]
    pub max_tokens: u32,

    /// 采样温度
    #[serde(default = "default_llm_temperature")]
    pub temperature: f32,
}

fn default_llm_provider() -> String {
    "deepseek".to_string()
}
fn default_llm_url() -> String {
    "https://api.deepseek.com/v1".to_string()
}
fn default_llm_model() -> String {
    "deepseek-chat".to_string()
}
fn default_think_level_str() -> String {
    "medium".to_string()
}
fn default_llm_timeout() -> u64 {
    120
}
fn default_llm_max_tokens() -> u32 {
    4096
}
fn default_llm_temperature() -> f32 {
    0.3
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_llm_provider(),
            url: default_llm_url(),
            api_key: String::new(),
            model: default_llm_model(),
            think_level: default_think_level_str(),
            timeout_secs: default_llm_timeout(),
            max_tokens: default_llm_max_tokens(),
            temperature: default_llm_temperature(),
        }
    }
}

/// 展开 `$NAME` / `${NAME}` 形式的环境变量引用。
///
/// - 非引用形式（如明文密钥）返回 `None`，调用方保持原值
/// - 引用形式但环境变量未设置时同样返回 `None`，保持原值，避免误替换
fn expand_env_var(value: &str) -> Option<String> {
    let name = if let Some(rest) = value.strip_prefix("${") {
        rest.strip_suffix('}')?
    } else if let Some(rest) = value.strip_prefix('$') {
        rest
    } else {
        return None;
    };
    if name.is_empty() {
        return None;
    }
    std::env::var(name).ok()
}

impl LlmConfig {
    /// 解析 API Key 中的环境变量引用。
    ///
    /// 支持在 config.toml 中通过 `$NAME` 或 `${NAME}` 形式引用环境变量，
    /// 避免明文密钥落盘，例如：
    ///
    /// ```toml
    /// [llm]
    /// api_key = "$MEMREC_LLM_KEY"
    /// ```
    ///
    /// 环境变量未设置时保持原值不变，由调用方按未配置处理。
    pub fn resolve_env_api_key(&mut self) {
        if let Some(resolved) = expand_env_var(&self.api_key) {
            self.api_key = resolved;
        }
    }

    /// 解析思考等级；非法值回退为 `Medium`。
    pub fn think_level(&self) -> ThinkLevel {
        match self.think_level.to_lowercase().as_str() {
            "off" => ThinkLevel::Off,
            "minimal" => ThinkLevel::Minimal,
            "low" => ThinkLevel::Low,
            "medium" => ThinkLevel::Medium,
            "high" => ThinkLevel::High,
            "xhigh" => ThinkLevel::Xhigh,
            _ => ThinkLevel::Medium,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_think_level_default() {
        assert_eq!(ThinkLevel::default(), ThinkLevel::Medium);
    }

    #[test]
    fn test_think_level_serde() {
        let json = r#""xhigh""#;
        let level: ThinkLevel = serde_json::from_str(json).unwrap();
        assert_eq!(level, ThinkLevel::Xhigh);

        let json = serde_json::to_string(&ThinkLevel::Minimal).unwrap();
        assert_eq!(json, r#""minimal""#);
    }

    #[test]
    fn test_deepseek_mapping() {
        assert_eq!(
            ThinkLevel::Medium.to_provider_param("deepseek"),
            Some(serde_json::json!({"thinking": {"type": "enabled", "effort": "high"}}))
        );
        assert_eq!(
            ThinkLevel::Xhigh.to_provider_param("deepseek"),
            Some(serde_json::json!({"thinking": {"type": "enabled", "effort": "max"}}))
        );
        assert_eq!(ThinkLevel::Off.to_provider_param("deepseek"), None);
    }

    #[test]
    fn test_openai_mapping() {
        assert_eq!(
            ThinkLevel::Medium.to_provider_param("openai"),
            Some(serde_json::json!({"reasoning_effort": "medium"}))
        );
        assert_eq!(
            ThinkLevel::Xhigh.to_provider_param("openai"),
            Some(serde_json::json!({"reasoning_effort": "high"}))
        );
        assert_eq!(ThinkLevel::Off.to_provider_param("openai"), None);
    }

    #[test]
    fn test_unknown_provider_no_thinking() {
        assert_eq!(ThinkLevel::High.to_provider_param("ollama"), None);
    }

    #[test]
    fn test_config_defaults() {
        let config = LlmConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.provider, "deepseek");
        assert_eq!(config.think_level(), ThinkLevel::Medium);
        assert_eq!(config.max_tokens, 4096);
    }

    #[test]
    fn test_config_partial_deserialize() {
        let json = r#"{
            "enabled": true,
            "provider": "deepseek",
            "url": "https://api.deepseek.com/v1",
            "apiKey": "sk-test",
            "model": "deepseek-chat",
            "thinkLevel": "xhigh"
        }"#;
        let config: LlmConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.api_key, "sk-test");
        assert_eq!(config.think_level(), ThinkLevel::Xhigh);
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_think_level_parse_fallback() {
        let mut config = LlmConfig::default();
        config.think_level = "invalid".to_string();
        assert_eq!(config.think_level(), ThinkLevel::Medium);

        config.think_level = "OFF".to_string();
        assert_eq!(config.think_level(), ThinkLevel::Off);
    }

    #[test]
    fn test_resolve_env_api_key_dollar_form() {
        std::env::set_var("MEMREC_TEST_LLM_KEY_DOLLAR", "sk-env-value");
        let mut config = LlmConfig::default();
        config.api_key = "$MEMREC_TEST_LLM_KEY_DOLLAR".to_string();
        config.resolve_env_api_key();
        assert_eq!(config.api_key, "sk-env-value");
        std::env::remove_var("MEMREC_TEST_LLM_KEY_DOLLAR");
    }

    #[test]
    fn test_resolve_env_api_key_braced_form() {
        std::env::set_var("MEMREC_TEST_LLM_KEY_BRACED", "sk-env-value");
        let mut config = LlmConfig::default();
        config.api_key = "${MEMREC_TEST_LLM_KEY_BRACED}".to_string();
        config.resolve_env_api_key();
        assert_eq!(config.api_key, "sk-env-value");
        std::env::remove_var("MEMREC_TEST_LLM_KEY_BRACED");
    }

    #[test]
    fn test_resolve_env_api_key_plain_unchanged() {
        let mut config = LlmConfig::default();
        config.api_key = "sk-plain-text".to_string();
        config.resolve_env_api_key();
        assert_eq!(config.api_key, "sk-plain-text");
    }

    #[test]
    fn test_resolve_env_api_key_unset_keeps_original() {
        std::env::remove_var("MEMREC_TEST_LLM_KEY_UNSET");
        let mut config = LlmConfig::default();
        config.api_key = "$MEMREC_TEST_LLM_KEY_UNSET".to_string();
        config.resolve_env_api_key();
        assert_eq!(config.api_key, "$MEMREC_TEST_LLM_KEY_UNSET");
    }

    #[test]
    fn test_resolve_env_api_key_empty_keeps_original() {
        let mut config = LlmConfig::default();
        config.api_key = "$".to_string();
        config.resolve_env_api_key();
        assert_eq!(config.api_key, "$");
    }
}
