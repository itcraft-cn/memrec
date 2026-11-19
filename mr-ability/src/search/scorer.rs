//! # 搜索评分器
//!
//! 计算最终搜索分数，包括时间衰减和源权重。

use chrono::{DateTime, Utc};
use mr_common::{MemoryScope, MemorySource};

/// 评分器配置。
#[derive(Debug, Clone)]
pub struct ScorerConfig {
    /// 时间衰减半衰期（小时）
    pub decay_half_life_hours: f64,
    /// 豁免衰减的作用域
    pub evergreen_scopes: Vec<MemoryScope>,
    /// 来源权重
    pub source_weights: SourceWeights,
    /// 新近修正强度（0.0-1.0）：最多把排序分折损该比例。
    ///
    /// 仅作用于**情景类**记忆（`conversation` / `context`）。语义类记忆
    /// （知识、决策、偏好）是"仍然成立的结论"，不因时间流逝而降权：
    /// 否则金标集里"旧但权威"的记忆会被"新但勉强相关"的条目挤出 top-k。
    pub recency_weight: f64,
    /// 重要性修正强度（0.0-1.0）：仅作同量级内的微调（tie-break）。
    pub importance_weight: f64,
    /// 来源修正强度（0.0-1.0）：仅作同量级内的微调（tie-break）。
    pub source_weight_strength: f64,
}

impl Default for ScorerConfig {
    fn default() -> Self {
        Self {
            decay_half_life_hours: 336.0,
            // T4: Workspace 已废弃（迁移归一为 Global），豁免集合仅保留 Global
            evergreen_scopes: vec![MemoryScope::Global],
            source_weights: SourceWeights::default(),
            recency_weight: 0.10,
            importance_weight: 0.02,
            source_weight_strength: 0.02,
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

/// 相关度之上的排序修饰（新近 + 重要性 + 来源）。
///
/// 与 [`apply_scoring`] 的区别：这里每个维度都是**有界折损**，只做同量级内的
/// 排序微调，不改变相关度的量纲与阈值语义。设计约束：
///
/// - 合成因子落在 `(1 - recency_weight - importance_weight -
///   source_weight_strength, 1]`，默认配置下不低于 0.86；
/// - 新近折损**只作用于情景类记忆**（见 [`is_episodic`]），语义类记忆不因
///   时间流逝降权；`Global` 作用域同样豁免；
/// - 重要性与来源是 tie-break 量级：默认各 2%，只能改变"分数几乎相同"的
///   候选次序，不能把真实相关度差距覆盖掉；
/// - `importance = 1.0`、`source = User` 时对应因子为 1.0，不产生额外增益，
///   保证“分数上限仍是 1.0”这一对外约定；
/// - 修正只应用于排序，`min_score` 的绝对阈值判定在此前完成。
pub fn apply_ranking_modifier(
    base: f64,
    created_at: DateTime<Utc>,
    scope: MemoryScope,
    source: MemorySource,
    importance: f32,
    memory_type: mr_common::MemoryType,
    config: &ScorerConfig,
) -> f64 {
    if base <= 0.0 {
        return base;
    }

    let mut factor = 1.0;

    // 新近只作用于情景类记忆：它们描述"何时发生了什么"，过期即失真；
    // 语义类记忆（知识/决策/偏好）是可复用结论，不随时间降权。
    if is_episodic(memory_type) && !config.evergreen_scopes.contains(&scope) {
        let decay = 0.5_f64.powf(
            (Utc::now() - created_at).num_hours().max(0) as f64 / config.decay_half_life_hours,
        );
        factor *= 1.0 - config.recency_weight * (1.0 - decay.clamp(0.0, 1.0));
    }

    let importance = (importance as f64).clamp(0.0, 1.0);
    factor *= 1.0 - config.importance_weight * (1.0 - importance);

    let weight = match source {
        MemorySource::User => config.source_weights.user,
        MemorySource::System => config.source_weights.system,
        MemorySource::Inferred => config.source_weights.inferred,
        MemorySource::External => config.source_weights.external,
    }
    .clamp(0.0, 1.0);
    factor *= 1.0 - config.source_weight_strength * (1.0 - weight);

    base * factor
}

/// 情景类记忆判据：描述"某时某地发生了什么"，会随时间失真。
///
/// `Conversation`（对话记录）与 `Context`（项目/环境上下文）属于此类；
/// `Knowledge` / `Decision` / `Preference` 是可复用结论，不参与新近折损。
fn is_episodic(memory_type: mr_common::MemoryType) -> bool {
    matches!(
        memory_type,
        mr_common::MemoryType::Conversation | mr_common::MemoryType::Context
    )
}
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

    #[test]
    fn test_ranking_modifier_recent_beats_old() {
        let config = ScorerConfig::default();
        let old = Utc::now() - chrono::Duration::hours(config.decay_half_life_hours as i64 * 4);
        let recent = Utc::now();

        let old_score = apply_ranking_modifier(
            1.0,
            old,
            MemoryScope::Project,
            MemorySource::User,
            1.0,
            mr_common::MemoryType::Context,
            &config,
        );
        let recent_score = apply_ranking_modifier(
            1.0,
            recent,
            MemoryScope::Project,
            MemorySource::User,
            1.0,
            mr_common::MemoryType::Context,
            &config,
        );

        assert!(recent_score > old_score, "新记忆应获得更高排序分");
        // 有界：最多折损 recency_weight，不会把精确命中压到量级之外。
        assert!(old_score > 1.0 - config.recency_weight - 1e-6);
    }

    #[test]
    fn test_ranking_modifier_global_exempt_from_decay() {
        let config = ScorerConfig::default();
        let old = Utc::now() - chrono::Duration::hours(10_000);
        let score = apply_ranking_modifier(
            0.9,
            old,
            MemoryScope::Global,
            MemorySource::User,
            1.0,
            mr_common::MemoryType::Context,
            &config,
        );
        assert!((score - 0.9).abs() < 1e-6, "Global 作用域不参与时间折损");
    }

    #[test]
    fn test_ranking_modifier_importance_and_source_monotonic() {
        let config = ScorerConfig::default();
        let now = Utc::now();

        let high = apply_ranking_modifier(
            1.0,
            now,
            MemoryScope::Project,
            MemorySource::User,
            1.0,
            mr_common::MemoryType::Context,
            &config,
        );
        let low = apply_ranking_modifier(
            1.0,
            now,
            MemoryScope::Project,
            MemorySource::User,
            0.2,
            mr_common::MemoryType::Context,
            &config,
        );
        assert!(high > low, "重要性越高排序分越高");
        assert!(high <= 1.0 + 1e-6, "排序分不得超过 1.0");

        let inferred = apply_ranking_modifier(
            1.0,
            now,
            MemoryScope::Project,
            MemorySource::Inferred,
            1.0,
            mr_common::MemoryType::Context,
            &config,
        );
        assert!(inferred < high, "推断来源应低于用户来源");
    }

    #[test]
    fn test_ranking_modifier_semantic_memory_exempt_from_decay() {
        let config = ScorerConfig::default();
        let old = Utc::now() - chrono::Duration::hours(10_000);

        // 语义类（知识/决策/偏好）：旧记忆与新高记忆排序分相同。
        for memory_type in [
            mr_common::MemoryType::Knowledge,
            mr_common::MemoryType::Decision,
            mr_common::MemoryType::Preference,
        ] {
            let score = apply_ranking_modifier(
                0.9,
                old,
                MemoryScope::Project,
                MemorySource::User,
                1.0,
                memory_type,
                &config,
            );
            assert!(
                (score - 0.9).abs() < 1e-6,
                "{memory_type} 不应因时间流逝降权"
            );
        }
    }

    #[test]
    fn test_ranking_modifier_zero_base_is_zero() {
        let config = ScorerConfig::default();
        let score = apply_ranking_modifier(
            0.0,
            Utc::now(),
            MemoryScope::Project,
            MemorySource::User,
            0.8,
            mr_common::MemoryType::Context,
            &config,
        );
        assert_eq!(score, 0.0);
    }
}
