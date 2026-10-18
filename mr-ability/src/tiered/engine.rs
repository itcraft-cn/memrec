//! # 分层检索引擎
//!
//! 协调 TierAssigner 和 BudgetController，实现分层检索。

use anyhow::Result;
use mr_common::{
    EdgeHint, Memory, MemoryEdge, RepresentationTier, TierThresholds, TieredMemory,
    TieredSearchResult,
};
use uuid::Uuid;

use crate::storage::{EdgeStorage, MemoryStorage, TieredStorage};
use crate::tiered::{BudgetController, TierAssigner};

#[derive(Debug, Clone)]
pub struct TieredEngineConfig {
    pub max_tokens: u32,
    pub thresholds: TierThresholds,
}

impl Default for TieredEngineConfig {
    fn default() -> Self {
        TieredEngineConfig {
            max_tokens: 4000,
            thresholds: TierThresholds::default(),
        }
    }
}

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
        let budget = BudgetController::new(config.max_tokens);
        TieredEngine {
            memory_store,
            edge_store,
            tiered_store,
            assigner,
            budget,
        }
    }

    pub async fn build_tiered_result(
        &self,
        memory_ids: &[Uuid],
        scores: &[f32],
    ) -> Result<TieredSearchResult> {
        let mut results = Vec::with_capacity(memory_ids.len());

        for (memory_id, score) in memory_ids.iter().zip(scores.iter()) {
            if let Some(memory) = self.memory_store.get(memory_id).await? {
                let tiered = self.build_tiered_memory(memory, *score).await?;
                results.push(tiered);
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_tokens = self.budget.check_and_downgrade(&mut results);

        Ok(TieredSearchResult {
            total_results: results.len(),
            total_tokens,
            budget: self.budget.max_tokens(),
            results,
        })
    }

    async fn build_tiered_memory(&self, memory: Memory, score: f32) -> Result<TieredMemory> {
        let tier = self.assigner.assign(score);
        let (content, token_count) = self.generate_tier_content(&memory, tier).await?;

        let facet_themes = vec![];
        let edge_hints = self.build_edge_hints(&memory.id).await?;

        Ok(TieredMemory {
            memory_id: memory.id,
            tier,
            content,
            score,
            token_count,
            facet_themes,
            edge_hints,
        })
    }

    async fn generate_tier_content(
        &self,
        memory: &Memory,
        tier: RepresentationTier,
    ) -> Result<(String, u32)> {
        match tier {
            RepresentationTier::Full => {
                let tokens = BudgetController::estimate_token_count(&memory.content);
                Ok((memory.content.clone(), tokens))
            }
            RepresentationTier::Truncated => {
                let truncated = self.truncate_content(&memory.content, 200);
                let tokens = BudgetController::estimate_token_count(&truncated);
                Ok((truncated, tokens.min(200)))
            }
            RepresentationTier::Summary => {
                if let Some(summary) = self.tiered_store.get_summary(&memory.id).await? {
                    let tokens = BudgetController::estimate_token_count(&summary);
                    Ok((summary, tokens.min(80)))
                } else {
                    let truncated = self.truncate_content(&memory.content, 80);
                    Ok((truncated, 80))
                }
            }
            RepresentationTier::DenseProxy => {
                if let Some(keywords) = self.tiered_store.get_keywords(&memory.id).await? {
                    let proxy = format!("[{}]", keywords.join(", "));
                    Ok((proxy, 30))
                } else {
                    let tags = memory.tags.clone();
                    let proxy = if tags.is_empty() {
                        "[no keywords]".to_string()
                    } else {
                        format!("[{}]", tags.join(", "))
                    };
                    Ok((proxy, 30))
                }
            }
        }
    }

    fn truncate_content(&self, content: &str, max_tokens: u32) -> String {
        let char_limit = (max_tokens as f32 / 0.4) as usize;
        if content.chars().count() <= char_limit {
            return content.to_string();
        }

        let truncated: String = content.chars().take(char_limit).collect();
        let mut result = truncated;

        for end in (0..result.len()).rev() {
            let c = result.chars().nth(end);
            if let Some(c) = c {
                if c == '。' || c == '！' || c == '？' || c == '.' || c == '!' || c == '?' {
                    result = result.chars().take(end + 1).collect();
                    result.push_str("...");
                    return result;
                }
            }
        }

        result.push_str("...");
        result
    }

    async fn build_edge_hints(&self, memory_id: &Uuid) -> Result<Vec<EdgeHint>> {
        let edges = self.edge_store.neighbors(memory_id).await?;
        let mut hints = Vec::new();

        for edge in edges.iter().take(3) {
            let target_summary = self.get_target_summary(edge).await?;
            hints.push(EdgeHint {
                edge_type: edge.edge_type,
                target_summary,
                weight: edge.weight,
            });
        }

        Ok(hints)
    }

    async fn get_target_summary(&self, edge: &MemoryEdge) -> Result<String> {
        let target_id = edge.target_id;
        if let Some(summary) = self.tiered_store.get_summary(&target_id).await? {
            return Ok(summary);
        }

        if let Some(memory) = self.memory_store.get(&target_id).await? {
            let truncated = self.truncate_content(&memory.content, 50);
            return Ok(truncated);
        }

        Ok("unknown".to_string())
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
