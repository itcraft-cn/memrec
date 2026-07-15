//! # 重要性计算器
//!
//! 使用四维加权公式计算记忆重要性分数（0.0~1.0）：
//!
//! ```text
//! importance = w_recency × recency + w_frequency × frequency
//!           + w_semantic × semantic + w_explicit × explicit
//! ```
//!
//! ## 四个维度
//!
//! | 维度 | 含义 | 计算方式 |
//! |------|------|----------|
//! | **recency** | 时间衰减 | `e^(-λ × 天数)`，越近越重要 |
//! | **frequency** | 访问频率 | `ln(访问次数+1) / 归一化因子` |
//! | **semantic** | 语义标签 | 取标签中最高权重值 |
//! | **explicit** | 显式优先级 | 从 metadata["priority"] 读取 |

use chrono::{DateTime, Utc};
use memrec_common::{ImportanceConfig, Memory};
use std::collections::HashMap;

/// 重要性计算器，持有配置和标签权重表。
pub struct ImportanceCalculator {
    config: ImportanceConfig,
    tag_weights: HashMap<String, f32>,
}

impl ImportanceCalculator {
    /// 使用指定配置创建计算器，初始化默认标签权重。
    pub fn new(config: ImportanceConfig) -> Self {
        Self {
            config,
            tag_weights: Self::default_tag_weights(),
        }
    }

    /// 默认标签权重表。
    ///
    /// | 标签 | 权重 | 含义 |
    /// |------|------|------|
    /// | critical | 1.0 | 关键信息 |
    /// | decision | 0.9 | 决策记录 |
    /// | key | 0.8 | 关键知识 |
    /// | important | 0.7 | 重要内容 |
    /// | config | 0.6 | 配置信息 |
    /// | reference | 0.5 | 参考资料 |
    /// | note | 0.4 | 普通笔记 |
    /// | temporary | 0.2 | 临时内容 |
    /// | draft | 0.1 | 草稿 |
    fn default_tag_weights() -> HashMap<String, f32> {
        let mut weights: HashMap<String, f32> = HashMap::new();
        weights.insert("critical".to_string(), 1.0);
        weights.insert("decision".to_string(), 0.9);
        weights.insert("key".to_string(), 0.8);
        weights.insert("important".to_string(), 0.7);
        weights.insert("config".to_string(), 0.6);
        weights.insert("reference".to_string(), 0.5);
        weights.insert("note".to_string(), 0.4);
        weights.insert("temporary".to_string(), 0.2);
        weights.insert("draft".to_string(), 0.1);
        weights
    }

    /// 计算记忆的重要性分数，结果 clamp 到 [0.0, 1.0]。
    pub fn calculate(&self, memory: &Memory) -> f32 {
        let now = Utc::now();

        let recency = self.calculate_recency(memory.last_accessed, now);
        let frequency = self.calculate_frequency(memory.access_count);
        let semantic = self.calculate_semantic(&memory.tags);
        let explicit = self.calculate_explicit(&memory.metadata);

        let importance = self.config.weight_recency * recency
            + self.config.weight_frequency * frequency
            + self.config.weight_semantic * semantic
            + self.config.weight_explicit * explicit;

        importance.clamp(0.0, 1.0)
    }

    /// 时间衰减维度：`e^(-λ × 距上次访问天数)`。
    ///
    /// λ 越大衰减越快，默认 0.05（约 14 天半衰期）。
    fn calculate_recency(&self, last_accessed: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
        let days_since_access = (now - last_accessed).num_days() as f32;
        (-self.config.lambda * days_since_access).exp()
    }

    /// 访问频率维度：`ln(访问次数+1) / 归一化因子`。
    ///
    /// 对数增长避免高频访问记忆分数过高。
    fn calculate_frequency(&self, access_count: u32) -> f32 {
        ((access_count as f32 + 1.0).ln()) / self.config.frequency_normalize
    }

    /// 语义标签维度：取标签中最高权重值。
    ///
    /// 无标签时返回 0.5（中性），未知标签也按 0.5 处理。
    fn calculate_semantic(&self, tags: &[String]) -> f32 {
        if tags.is_empty() {
            return 0.5;
        }

        tags.iter()
            .map(|tag| self.tag_weights.get(tag).copied().unwrap_or(0.5))
            .fold(0.5, |max, val| if val > max { val } else { max })
    }

    /// 显式优先级维度：从 metadata["priority"] 读取。
    ///
    /// 无显式优先级时默认 0.5。
    fn calculate_explicit(&self, metadata: &HashMap<String, String>) -> f32 {
        metadata
            .get("priority")
            .and_then(|p| p.parse::<f32>().ok())
            .unwrap_or(0.5)
    }
}

impl Default for ImportanceCalculator {
    fn default() -> Self {
        Self::new(ImportanceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memrec_common::MemoryType;

    #[test]
    fn test_calculator_new() {
        let calc = ImportanceCalculator::default();

        assert_eq!(calc.config.lambda, 0.05);
        assert_eq!(calc.config.weight_recency, 0.3);
    }

    #[test]
    fn test_calculate_full() {
        let calc = ImportanceCalculator::default();

        let memory = Memory::new("test".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["critical".to_string()]);

        let importance = calc.calculate(&memory);

        // With critical tag (weight 1.0) and weight_semantic = 0.2
        // Expected: 0.3*recency + 0.2*frequency + 0.2*1.0 + 0.3*0.5 = ~0.6
        assert!(importance > 0.5);
    }
}
