//! # 搜索评分器
//!
//! 计算最终搜索分数，包括时间衰减和源权重。

use chrono::{DateTime, Utc};
use memrec_common::{MemoryScope, MemorySource};

/// 评分器配置。
#[derive(Debug, Clone)]
pub struct ScorerConfig {
    /// 时间衰减半衰期（小时）
    pub decay_half_life_hours: f64,
    /// 豁免衰减的作用域
    pub evergreen_scopes: Vec<MemoryScope>,
    /// 来源权重
    pub source_weights: SourceWeights,
}

impl Default for ScorerConfig {
    fn default() -> Self {
        Self {
            decay_half_life_hours: 336.0,
            evergreen_scopes: vec![MemoryScope::Global, MemoryScope::Workspace],
            source_weights: SourceWeights::default(),
        }
    }
}

/// 来源权重配置。
#[derive(Debug, Clone)]
pub struct SourceWeights {
    pub user: f64,
    pub system: f64,
    pub inferred: f64,
    pub external: f64,
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

/// 应用完整评分（时间衰减 + 源权重）。
pub fn apply_scoring(
    base_score: f64,
    created_at: DateTime<Utc>,
    scope: MemoryScope,
    source: MemorySource,
    config: &ScorerConfig,
) -> f64 {
    let decayed = apply_time_decay(base_score, created_at, scope, config);
    apply_source_weight(decayed, source, &config.source_weights)
}

/// 应用时间衰减。
fn apply_time_decay(
    score: f64,
    created_at: DateTime<Utc>,
    scope: MemoryScope,
    config: &ScorerConfig,
) -> f64 {
    if config.evergreen_scopes.contains(&scope) {
        return score;
    }

    let age_hours = (Utc::now() - created_at).num_hours() as f64;
    let lambda = std::f64::consts::LN_2 / config.decay_half_life_hours;
    score * (-lambda * age_hours).exp()
}

/// 应用源权重。
fn apply_source_weight(score: f64, source: MemorySource, weights: &SourceWeights) -> f64 {
    let w = match source {
        MemorySource::User => weights.user,
        MemorySource::System => weights.system,
        MemorySource::Inferred => weights.inferred,
        MemorySource::External => weights.external,
    };
    score * w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_decay_project_scope() {
        let config = ScorerConfig::default();
        let created_at = Utc::now() - chrono::Duration::hours(336);
        let decayed = apply_time_decay(1.0, created_at, MemoryScope::Project, &config);
        assert!((decayed - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_time_decay_global_scope_exempt() {
        let config = ScorerConfig::default();
        let created_at = Utc::now() - chrono::Duration::hours(336);
        let decayed = apply_time_decay(1.0, created_at, MemoryScope::Global, &config);
        assert!((decayed - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_source_weight_user() {
        let weights = SourceWeights::default();
        let weighted = apply_source_weight(1.0, MemorySource::User, &weights);
        assert!((weighted - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_source_weight_inferred() {
        let weights = SourceWeights::default();
        let weighted = apply_source_weight(1.0, MemorySource::Inferred, &weights);
        assert!((weighted - 0.5).abs() < 0.001);
    }
}
