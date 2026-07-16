//! # 混合搜索存储
//!
//! 整合向量检索（KNN）和全文检索（BM25），提供统一搜索接口。

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::traits::{
    FtsPayload, FtsStorage, HybridSearchRequest, HybridSearchResult, HybridStorage, SearchHit,
    VectorPayload, VectorStorage,
};
use crate::search::mmr::{MmrConfig, MmrHit, mmr_rerank};
use crate::search::scorer::ScorerConfig;

/// 混合搜索配置。
#[derive(Debug, Clone)]
pub struct HybridConfig {
    /// 向量检索权重（0.0-1.0）
    pub hybrid_alpha: f32,
    /// 是否启用 MMR 重排
    pub mmr_enabled: bool,
}

impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            hybrid_alpha: 0.5,
            mmr_enabled: true,
        }
    }
}

/// 混合搜索存储。
pub struct HybridStore {
    vector_store: Arc<dyn VectorStorage>,
    fts_store: Arc<dyn FtsStorage>,
    mmr_config: MmrConfig,
    scorer_config: ScorerConfig,
}

impl HybridStore {
    /// 创建新的混合搜索存储。
    pub fn new(
        vector_store: Arc<dyn VectorStorage>,
        fts_store: Arc<dyn FtsStorage>,
        mmr_config: MmrConfig,
        scorer_config: ScorerConfig,
    ) -> Self {
        Self {
            vector_store,
            fts_store,
            mmr_config,
            scorer_config,
        }
    }

    /// 合并向量和全文检索结果并归一化。
    fn merge_and_normalize(
        vec_hits: Vec<SearchHit>,
        fts_hits: Vec<SearchHit>,
        alpha: f32,
    ) -> Vec<SearchHit> {
        if vec_hits.is_empty() && fts_hits.is_empty() {
            return Vec::new();
        }

        let (vec_min, vec_max) = vec_hits
            .iter()
            .fold((f32::MAX, f32::MIN), |(min, max), h| {
                (min.min(h.score), max.max(h.score))
            });

        let (fts_min, fts_max) = fts_hits
            .iter()
            .fold((f32::MAX, f32::MIN), |(min, max), h| {
                (min.min(h.score), max.max(h.score))
            });

        let mut merged: HashMap<Uuid, SearchHit> = HashMap::new();

        for hit in vec_hits {
            let norm_score = if (vec_max - vec_min).abs() < f32::EPSILON {
                1.0
            } else {
                1.0 - (hit.score - vec_min) / (vec_max - vec_min)
            };
            let final_score = alpha * norm_score;
            merged.insert(
                hit.memory_id,
                SearchHit {
                    memory_id: hit.memory_id,
                    score: final_score,
                    payload: hit.payload,
                },
            );
        }

        for hit in fts_hits {
            let norm_score = if (fts_max - fts_min).abs() < f32::EPSILON {
                1.0
            } else {
                (hit.score - fts_min) / (fts_max - fts_min)
            };
            let fts_contrib = (1.0 - alpha) * norm_score;

            if let Some(existing) = merged.get_mut(&hit.memory_id) {
                existing.score += fts_contrib;
            } else {
                merged.insert(
                    hit.memory_id,
                    SearchHit {
                        memory_id: hit.memory_id,
                        score: fts_contrib,
                        payload: hit.payload,
                    },
                );
            }
        }

        let mut result: Vec<SearchHit> = merged.into_values().collect();
        result.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        result
    }
}

/// MMR 搜索命中包装。
#[derive(Clone)]
struct MmrSearchHit {
    inner: SearchHit,
}

impl MmrHit for MmrSearchHit {
    fn score(&self) -> f64 {
        self.inner.score as f64
    }
    fn text(&self) -> &str {
        &self.inner.payload.content_preview
    }
}

#[async_trait]
impl HybridStorage for HybridStore {
    async fn search(&self, req: HybridSearchRequest) -> Result<HybridSearchResult> {
        let (vec_result, fts_result) = tokio::join!(
            self.vector_store
                .search(&req.query_embedding, req.filter.clone(), req.top_k),
            self.fts_store.search(&req.query, req.filter, req.top_k)
        );

        let vec_hits = vec_result?;
        let fts_hits = fts_result?;

        let vec_count = vec_hits.len();
        let fts_count = fts_hits.len();

        let merged = Self::merge_and_normalize(vec_hits, fts_hits, req.hybrid_alpha);

        // 3. Apply scoring (time decay + source weight)
        // TODO: Need created_at/scope/source in VectorPayload or SearchHit for scoring
        // For now, skip scoring step - scorer_config is reserved for future use
        let _ = &self.scorer_config;

        let mut sorted = merged;
        sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(req.top_k);

        let final_hits = if req.mmr_enabled {
            let mmr_hits: Vec<MmrSearchHit> = sorted
                .into_iter()
                .map(|h| MmrSearchHit { inner: h })
                .collect();

            let mut config = self.mmr_config.clone();
            config.lambda = req.mmr_lambda as f64;
            let reranked = mmr_rerank(mmr_hits, &config);
            reranked.into_iter().map(|h| h.inner).collect()
        } else {
            sorted
        };

        Ok(HybridSearchResult {
            hits: final_hits,
            vec_count,
            fts_count,
        })
    }

    async fn add(
        &self,
        id: &Uuid,
        embedding: &[f32],
        text: &str,
        payload: VectorPayload,
    ) -> Result<()> {
        self.vector_store.add(id, embedding, payload.clone()).await?;

        let fts_payload = FtsPayload {
            project_id: payload.project_id,
            memory_type: payload.memory_type,
            tags: payload.tags,
            importance: payload.importance,
        };
        self.fts_store.add(id, text, fts_payload).await?;

        Ok(())
    }

    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let vec_removed = self.vector_store.remove(id).await?;
        let fts_removed = self.fts_store.remove(id).await?;
        Ok(vec_removed || fts_removed)
    }
}
