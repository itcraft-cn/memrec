//! # 记忆去重与更新配置
//!
//! 控制写入时的相似性检测行为：发现重复记忆时跳过，发现可合并记忆时更新。
//!
//! ## 设计说明
//!
//! 借鉴 Mem0 的 ADD/UPDATE/DELETE/NOOP 四操作理念，memrec 在写入侧提供
//! 规则版决策（无需 LLM，基于相似度阈值）：
//!
//! - `score >= duplicate_threshold`：视为重复（NOOP/ADD 的否定），跳过写入
//! - `update_threshold <= score < duplicate_threshold`：视为信息演进（UPDATE），
//!   合并更新已有记忆
//! - 低于 `update_threshold`：正常新增（ADD）
//!
//! DELETE（矛盾消解）需要语义理解，规则版不自动执行，保留给后续 LLM 决策演进。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 写入时的相似记忆候选。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarMemory {
    /// 已有记忆 ID
    pub memory_id: Uuid,
    /// 已有记忆内容
    pub content: String,
    /// 相似度分数（0.0 - 1.0）
    pub score: f32,
}

/// 写入去重配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupConfig {
    /// 是否启用写入时相似性检测，默认 true
    #[serde(default = "default_dedup_enabled")]
    pub enabled: bool,

    /// 检索的相似记忆数量，默认 5
    #[serde(default = "default_dedup_top_k")]
    pub top_k: usize,

    /// 视为重复的相似度阈值，默认 0.92
    /// 达到此阈值直接跳过写入
    #[serde(default = "default_duplicate_threshold")]
    pub duplicate_threshold: f32,

    /// 视为可更新合并的相似度阈值，默认 0.85
    /// 达到此阈值更新已有记忆（合并 tags，内容取新）
    #[serde(default = "default_update_threshold")]
    pub update_threshold: f32,

    /// 原地更新窗口（分钟）。仅当已有记忆的年龄不超过该窗口时，才允许
    /// 写入路径原地合并更新；超出窗口的"修正"改为新增并把旧记录标记为
    /// `superseded`（保留历史，不做覆盖）。默认 60。
    ///
    /// 置 0 表示不限制（始终允许原地更新），用于向后兼容或显式放开。
    #[serde(default = "default_update_window_mins")]
    pub update_window_mins: u32,
}

fn default_update_window_mins() -> u32 {
    60
}

fn default_dedup_enabled() -> bool {
    true
}
fn default_dedup_top_k() -> usize {
    5
}
fn default_duplicate_threshold() -> f32 {
    0.92
}
fn default_update_threshold() -> f32 {
    0.85
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            enabled: default_dedup_enabled(),
            top_k: default_dedup_top_k(),
            duplicate_threshold: default_duplicate_threshold(),
            update_threshold: default_update_threshold(),
            update_window_mins: default_update_window_mins(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_config_defaults() {
        let config = DedupConfig::default();
        assert!(config.enabled);
        assert_eq!(config.top_k, 5);
        assert_eq!(config.duplicate_threshold, 0.92);
        assert_eq!(config.update_threshold, 0.85);
        assert_eq!(config.update_window_mins, 60);
    }

    #[test]
    fn test_dedup_config_partial_deserialize() {
        let json = r#"{"duplicate_threshold": 0.95}"#;
        let config: DedupConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.duplicate_threshold, 0.95);
        assert_eq!(config.update_threshold, 0.85);
    }

    #[test]
    fn test_similar_memory_serde() {
        let similar = SimilarMemory {
            memory_id: Uuid::nil(),
            content: "test".to_string(),
            score: 0.9,
        };
        let json = serde_json::to_string(&similar).unwrap();
        let parsed: SimilarMemory = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.memory_id, Uuid::nil());
        assert_eq!(parsed.score, 0.9);
    }
}
