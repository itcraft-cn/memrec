//! 检索与注入侧的运行时配置。
//!
//! 覆盖两类能力：
//!
//! 1. **访问反馈**（`access_recording` / `access_flush_secs`）：检索命中的记忆由
//!    后台任务批量回写 `last_accessed` / `access_count`，避免在请求链路上写存储，
//!    同时让重要性模型的时效/频率维度获得真实信号。
//! 2. **注入预算**（`injection_max_tokens` / `injection_type_caps`）：调用方未显式
//!    指定预算时使用的默认总预算，以及按记忆类型分配预算的上限规则。

use serde::{Deserialize, Serialize};

use crate::types::{TypeBudgetCap, TypeBudgetPolicy};

/// 检索与注入配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalConfig {
    /// 是否记录检索访问（异步批量回写，默认开启）。
    #[serde(default = "default_access_recording")]
    pub access_recording: bool,
    /// 访问回写的批量冲刷间隔（秒），默认 5。
    #[serde(default = "default_access_flush_secs")]
    pub access_flush_secs: u64,
    /// 分层注入的默认总 token 预算，默认 4000。
    ///
    /// 仅作为服务端默认值；调用方可通过请求参数覆盖。
    #[serde(default = "default_injection_max_tokens")]
    pub injection_max_tokens: u32,
    /// 注入预算的类型上限规则（按优先级排列），默认 preference 0.5 / decision 0.3 / knowledge 0.2。
    #[serde(default = "default_injection_type_caps")]
    pub injection_type_caps: Vec<TypeBudgetCap>,
}

fn default_access_recording() -> bool {
    true
}

fn default_access_flush_secs() -> u64 {
    5
}

fn default_injection_max_tokens() -> u32 {
    4000
}

fn default_injection_type_caps() -> Vec<TypeBudgetCap> {
    TypeBudgetPolicy::default().caps
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            access_recording: default_access_recording(),
            access_flush_secs: default_access_flush_secs(),
            injection_max_tokens: default_injection_max_tokens(),
            injection_type_caps: default_injection_type_caps(),
        }
    }
}
