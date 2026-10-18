//! 检索配置。

use serde::{Deserialize, Serialize};
use super::*;

/// 搜索配置。
///
/// 控制混合检索、MMR 重排、时间衰减等行为。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// 向量检索权重，默认 0.5（BM25 权重为 1 - alpha）
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,

    /// 是否启用 MMR 重排
    #[serde(default = "default_mmr_enabled")]
    pub mmr_enabled: bool,

    /// MMR 相关性权重，默认 0.5
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,

    /// 时间衰减半衰期（小时），默认 336（14天）
    #[serde(default = "default_decay_half_life")]
    pub decay_half_life_hours: f64,

    /// 来源权重
    #[serde(default)]
    pub source_weights: SourceWeights,

    /// 豁免时间衰减的作用域
    #[serde(default = "default_evergreen_scopes")]
    pub evergreen_scopes: Vec<String>,
}


fn default_hybrid_alpha() -> f64 {
    0.5
}


fn default_mmr_enabled() -> bool {
    true
}


fn default_mmr_lambda() -> f64 {
    0.5
}


fn default_decay_half_life() -> f64 {
    336.0
}


fn default_evergreen_scopes() -> Vec<String> {
    vec!["global".to_string(), "workspace".to_string()]
}


impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            hybrid_alpha: 0.5,
            mmr_enabled: true,
            mmr_lambda: 0.5,
            decay_half_life_hours: 336.0,
            source_weights: SourceWeights::default(),
            evergreen_scopes: vec!["global".to_string(), "workspace".to_string()],
        }
    }
}

