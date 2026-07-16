//! # 系统配置类型
//!
//! 定义记忆管理策略、重要性计算权重、搜索配置、服务端路径等配置结构。
//!
//! ## 配置层次
//!
//! - [`MemoryConfig`]：记忆生命周期管理策略（软删除恢复期、硬删除阈值、存储水位线等）
//! - [`ImportanceConfig`]：重要性计算公式的权重参数（时效性/频率/语义/显式评分）
//! - [`SearchConfig`]：搜索行为配置（混合检索权重、MMR 重排、时间衰减、来源权重）
//! - [`ServerConfig`]：服务端路径配置（Unix Socket 路径、数据目录）
//!
//! 这些配置通过 `config.toml` 持久化，由 [`memrecd::config::DaemonConfig`] 加载。

use serde::{Deserialize, Serialize};

/// 记忆生命周期管理配置。
///
/// 控制记忆的软删除恢复、硬删除淘汰、存储压缩等策略。
/// 水位线机制：当存储使用率达到 `high_watermark` 时触发淘汰，降至 `low_watermark` 时停止。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// 软删除后的恢复天数，超过此期限将无法恢复
    pub soft_delete_recovery_days: u32,
    /// 重要性低于此阈值的记忆可被硬删除淘汰
    pub hard_delete_importance: f32,
    /// 超过此天数未访问的低重要性记忆可被淘汰
    pub hard_delete_inactive_days: u32,
    /// 重要性低于此阈值的记忆可被压缩
    pub compression_importance: f32,
    /// 最大存储容量（GB）
    pub max_storage_gb: usize,
    /// 高水位线，存储使用率达到此值时触发淘汰
    pub high_watermark: f32,
    /// 低水位线，淘汰至此值时停止
    pub low_watermark: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            soft_delete_recovery_days: 30,
            hard_delete_importance: 0.1,
            hard_delete_inactive_days: 90,
            compression_importance: 0.3,
            max_storage_gb: 10,
            high_watermark: 0.9,
            low_watermark: 0.7,
        }
    }
}

/// 重要性计算权重配置。
///
/// 重要性评分公式为各维度加权和：
/// `score = weight_recency * recency + weight_frequency * frequency + weight_semantic * semantic + weight_explicit * explicit`
///
/// `lambda` 控制时效性衰减速率，`frequency_normalize` 控制频率归一化基数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportanceConfig {
    /// 时效性衰减系数，值越大衰减越快
    pub lambda: f32,
    /// 频率归一化基数，访问次数除以此值得到归一化频率
    pub frequency_normalize: f32,
    /// 时效性维度权重
    pub weight_recency: f32,
    /// 访问频率维度权重
    pub weight_frequency: f32,
    /// 语义相关性维度权重
    pub weight_semantic: f32,
    /// 显式评分维度权重（用户通过 tag 重要性标记）
    pub weight_explicit: f32,
}

impl Default for ImportanceConfig {
    fn default() -> Self {
        Self {
            lambda: 0.05,
            frequency_normalize: 10.0,
            weight_recency: 0.3,
            weight_frequency: 0.2,
            weight_semantic: 0.2,
            weight_explicit: 0.3,
        }
    }
}

/// 记忆来源权重配置。
///
/// 不同来源的记忆可信度不同，在搜索评分中体现。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceWeights {
    #[serde(default = "default_user_weight")]
    pub user: f64,
    #[serde(default = "default_system_weight")]
    pub system: f64,
    #[serde(default = "default_inferred_weight")]
    pub inferred: f64,
    #[serde(default = "default_external_weight")]
    pub external: f64,
}

fn default_user_weight() -> f64 {
    1.0
}
fn default_system_weight() -> f64 {
    0.8
}
fn default_inferred_weight() -> f64 {
    0.5
}
fn default_external_weight() -> f64 {
    0.7
}

impl Default for SourceWeights {
    fn default() -> Self {
        Self {
            user: 1.0,
            system: 0.8,
            inferred: 0.5,
            external: 0.7,
        }
    }
}

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

/// 服务端路径配置。
///
/// 路径中可包含 `~` 前缀，由守护进程在加载时通过 `shellexpand` 展开。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Unix Socket 监听路径
    pub socket_path: String,
    /// RocksDB 数据存储目录
    pub data_dir: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            socket_path: "~/.memrec/memrecd.sock".to_string(),
            data_dir: "~/.memrec/data".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_config_defaults() {
        let config = MemoryConfig::default();

        assert_eq!(config.soft_delete_recovery_days, 30);
        assert_eq!(config.hard_delete_importance, 0.1);
        assert_eq!(config.max_storage_gb, 10);
        assert_eq!(config.high_watermark, 0.9);
    }

    #[test]
    fn test_importance_config_defaults() {
        let config = ImportanceConfig::default();

        assert_eq!(config.lambda, 0.05);
        assert_eq!(config.weight_recency, 0.3);
    }

    #[test]
    fn test_config_serde() {
        let config = MemoryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: MemoryConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            config.soft_delete_recovery_days,
            parsed.soft_delete_recovery_days
        );
    }
}
