//! 记忆管理与重要性、来源权重配置。

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

