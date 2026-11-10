//! # 分层检索引擎
//!
//! 协调 [`TierAssigner`] 与 [`BudgetController`]，把混合检索的候选转成
//! “按完整度分层、受总预算与类型优先级约束”的可注入结果。
//!
//! ## 处理顺序
//!
//! 1. 批量回表取记忆（一次 `get_many`，避免 N+1）；
//! 2. 按分数分配初始层级（Full/Truncated/Summary/DenseProxy）；
//! 3. 预算分配：先降级、后丢弃，按记忆类型优先级占用预算；
//! 4. 仅对最终保留的条目渲染内容，保证 `token_count` 与实际内容一致。
//!
//! 第 4 步很关键：旧实现先按初始层级渲染、再在预算阶段改 tier 而不改内容，
//! 会出现“标注为 Summary、实际塞进完整正文”的预算失真。

use anyhow::Result;
use mr_common::{
    EdgeHint, Memory, MemoryEdge, RepresentationTier, TierThresholds, TieredMemory,
    TieredSearchResult, TypeBudgetPolicy,
};
use std::collections::HashMap;
use uuid::Uuid;

use crate::storage::{EdgeStorage, MemoryStorage, TieredStorage};
use crate::tiered::budget::{truncate_to_tokens, BudgetController, BudgetItem};
use crate::tiered::TierAssigner;

/// 分层引擎配置。
#[derive(Debug, Clone)]
pub struct TieredEngineConfig {
    pub max_tokens: u32,
    pub thresholds: TierThresholds,
    /// 类型预算优先级（默认 preference > decision > knowledge）
    pub policy: TypeBudgetPolicy,
}

impl Default for TieredEngineConfig {
    fn default() -> Self {
        TieredEngineConfig {
            max_tokens: 4000,
            thresholds: TierThresholds::default(),
            policy: TypeBudgetPolicy::default(),
        }
    }
}

/// 构造分层结果（自由函数版）。
///
/// 供 [`TieredEngine`] 与守护进程的检索处理器共用，避免两套实现漂移。
/// `memory_ids` 与 `scores` 按位置一一对应，长度不一致时以较短者为准。
#[allow(clippy::too_many_arguments)]
pub async fn build_tiered_result(
    storage: &dyn MemoryStorage,
    edge_store: Option<&dyn EdgeStorage>,
    tiered_store: Option<&dyn TieredStorage>,
    assigner: &TierAssigner,
    budget: &BudgetController,
    memory_ids: &[Uuid],
    scores: &[f32],
) -> Result<TieredSearchResult> {
    let n = memory_ids.len().min(scores.len());
    let ids = &memory_ids[..n];

    let fetched = storage.get_many(ids).await?;
    let mut by_id: HashMap<Uuid, Memory> = HashMap::with_capacity(n);
    for memory in fetched.into_iter().flatten() {
        if !memory.is_deleted {
            by_id.insert(memory.id, memory);
        }
    }

    let items: Vec<BudgetItem> = ids
        .iter()
        .zip(scores.iter())
        .filter_map(|(id, score)| {
            by_id.get(id).map(|memory| BudgetItem {
                memory_id: *id,
                memory_type: memory.memory_type,
                score: *score,
                tier: assigner.assign(*score),
                full_tokens: BudgetController::estimate_token_count(&memory.content),
            })
        })
        .collect();

    let selected = budget.allocate(items);
    let mut results = Vec::with_capacity(selected.len());

    for item in &selected {
        let Some(memory) = by_id.get(&item.memory_id) else {
            continue;
        };
        let (content, token_count) = render_tier_content(memory, item.tier, tiered_store).await?;
        let edge_hints = match edge_store {
            Some(store) => build_edge_hints(store, storage, tiered_store, &memory.id).await?,
            None => Vec::new(),
        };
        results.push(TieredMemory {
            memory_id: memory.id,
            tier: item.tier,
            content,
            score: item.score,
            token_count,
            memory_type: memory.memory_type,
            tags: memory.tags.clone(),
            facet_themes: Vec::new(),
            edge_hints,
        });
    }

    let total_tokens = results.iter().map(|r| r.token_count).sum();
    Ok(TieredSearchResult {
        total_results: results.len(),
        total_tokens,
        budget: budget.max_tokens(),
        results,
    })
}

/// 按层级渲染内容，并返回实际 token 估算。
async fn render_tier_content(
    memory: &Memory,
    tier: RepresentationTier,
    tiered_store: Option<&dyn TieredStorage>,
) -> Result<(String, u32)> {
    let content = match tier {
        RepresentationTier::Full => memory.content.clone(),
        RepresentationTier::Truncated => truncate_to_tokens(&memory.content, 200),
        RepresentationTier::Summary => {
            let summary = match tiered_store {
                Some(store) => store.get_summary(&memory.id).await?,
                None => None,
            };
            match summary {
                Some(summary) => truncate_to_tokens(&summary, 80),
                None => truncate_to_tokens(&memory.content, 80),
            }
        }
        RepresentationTier::DenseProxy => {
            let keywords = match tiered_store {
                Some(store) => store.get_keywords(&memory.id).await?,
                None => None,
            };
            let proxy = match keywords {
                Some(kw) if !kw.is_empty() => format!("[{}]", kw.join(", ")),
                _ if !memory.tags.is_empty() => format!("[{}]", memory.tags.join(", ")),
                _ => "[no keywords]".to_string(),
            };
            truncate_to_tokens(&proxy, 30)
        }
    };

    let token_count = BudgetController::estimate_token_count(&content);
    Ok((content, token_count))
}

/// 构造最多 3 条一跳边的提示。
async fn build_edge_hints(
    edge_store: &dyn EdgeStorage,
    storage: &dyn MemoryStorage,
    tiered_store: Option<&dyn TieredStorage>,
    memory_id: &Uuid,
) -> Result<Vec<EdgeHint>> {
    let edges = edge_store.neighbors(memory_id).await?;
    let mut hints = Vec::new();
    for edge in edges.iter().take(3) {
        hints.push(EdgeHint {
            edge_type: edge.edge_type,
            target_summary: target_summary(edge, storage, tiered_store).await?,
            weight: edge.weight,
        });
    }
    Ok(hints)
}

async fn target_summary(
    edge: &MemoryEdge,
    storage: &dyn MemoryStorage,
    tiered_store: Option<&dyn TieredStorage>,
) -> Result<String> {
    if let Some(store) = tiered_store {
        if let Some(summary) = store.get_summary(&edge.target_id).await? {
            return Ok(summary);
        }
    }
    match storage.get(&edge.target_id).await? {
        Some(memory) => Ok(truncate_to_tokens(&memory.content, 50)),
        None => Ok("unknown".to_string()),
    }
}

/// 分层检索引擎。
pub struct TieredEngine<M, E, T>
where
    M: MemoryStorage,
    E: EdgeStorage,
    T: TieredStorage,
{
    memory_store: M,
    edge_store: E,
    tiered_store: T,
    assigner: TierAssigner,
    budget: BudgetController,
}

impl<M, E, T> TieredEngine<M, E, T>
where
    M: MemoryStorage,
    E: EdgeStorage,
    T: TieredStorage,
{
    pub fn new(
        memory_store: M,
        edge_store: E,
        tiered_store: T,
        config: TieredEngineConfig,
    ) -> Self {
        let assigner = TierAssigner::new(config.thresholds);
        let budget = BudgetController::new(config.max_tokens).with_policy(config.policy);
        TieredEngine {
            memory_store,
            edge_store,
            tiered_store,
            assigner,
            budget,
        }
    }

    /// 构造分层结果。
    pub async fn build_tiered_result(
        &self,
        memory_ids: &[Uuid],
        scores: &[f32],
    ) -> Result<TieredSearchResult> {
        build_tiered_result(
            &self.memory_store,
            Some(&self.edge_store),
            Some(&self.tiered_store),
            &self.assigner,
            &self.budget,
            memory_ids,
            scores,
        )
        .await
    }

    /// 按 token 预算截断文本（保留句子边界）。
    pub fn truncate_content(&self, content: &str, max_tokens: u32) -> String {
        truncate_to_tokens(content, max_tokens)
    }

    pub fn assigner(&self) -> &TierAssigner {
        &self.assigner
    }

    pub fn budget(&self) -> &BudgetController {
        &self.budget
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
