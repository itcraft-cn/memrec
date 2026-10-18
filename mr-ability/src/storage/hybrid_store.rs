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
    TextProvider, VectorPayload, VectorStorage,
};
use crate::search::mmr::{mmr_rerank, MmrConfig, MmrHit};

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
    /// 重排文本提供者：MMR 相似度用全文而非 200 字符 content_preview，
    /// 避免"开头相同、结论相反"的记忆被误判相似（与多样性目标相反）。
    text_provider: Option<Arc<dyn TextProvider>>,
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
            text_provider: None,
        }
    }

    /// 附加重排文本提供者（builder 风格，不破坏既有构造签名）。
    pub fn with_text_provider(mut self, provider: Arc<dyn TextProvider>) -> Self {
        self.text_provider = Some(provider);
        self
    }

    /// 重载全文索引读取器，用于测试场景。
    pub fn reload(&self) -> Result<()> {
        self.fts_store.reload()
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

        let (vec_min, vec_max) = vec_hits.iter().fold((f32::MAX, f32::MIN), |(min, max), h| {
            (min.min(h.score), max.max(h.score))
        });

        let (fts_min, fts_max) = fts_hits.iter().fold((f32::MAX, f32::MIN), |(min, max), h| {
            (min.min(h.score), max.max(h.score))
        });

        let mut merged: HashMap<Uuid, SearchHit> = HashMap::new();

        for hit in vec_hits {
            // 向量侧 score 为余弦相似度（越大越相似），归一化后应保持同向：
            // 最相似项 -> 1.0，最不相似项 -> 0.0。此前写成 `1.0 - ...` 方向相反，
            // 导致最相关记忆被压到最低分并垫底。
            let norm_score = if (vec_max - vec_min).abs() < f32::EPSILON {
                1.0
            } else {
                (hit.score - vec_min) / (vec_max - vec_min)
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
        // 确定性排序：分数降序，同分按 memory_id 升序，避免 HashMap 迭代顺序导致
        // 同分命中排名在多次运行间漂移（影响 MRR/nDCG 的可复现性）。
        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.memory_id.cmp(&b.memory_id))
        });
        result
    }
}

/// MMR 搜索命中包装。
#[derive(Clone)]
struct MmrSearchHit {
    inner: SearchHit,
    /// 重排用全文：TextProvider 提供；缺失时回退 content_preview。
    rerank_text: String,
}

impl MmrHit for MmrSearchHit {
    fn score(&self) -> f64 {
        self.inner.score as f64
    }
    fn text(&self) -> &str {
        &self.rerank_text
    }
}

#[async_trait]
impl HybridStorage for HybridStore {
    async fn search(&self, req: HybridSearchRequest) -> Result<HybridSearchResult> {
        let filter = req.filter.clone();
        let (vec_result, fts_result) = tokio::join!(
            self.vector_store
                .search(&req.query_embedding, filter.clone(), req.top_k),
            self.fts_store.search(&req.query, filter.clone(), req.top_k)
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
        sorted.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.memory_id.cmp(&b.memory_id))
        });
        // 量纲分离：`min_score` 只约束"原始向量余弦相似度"，由向量存储在 KNN 阶段
        // 过滤（[`VectorStorage::search`]）。融合分是 min-max 归一化后再按 alpha 加权的
        // **相对量**，与原始相似度不可比；此前对融合结果再套同一阈值做绝对截断，会产生
        // 两个后果：纯向量命中的顶项恒等于 alpha（默认 0.5），恰与默认 min_score 相等，
        // 于是结果被过度裁剪到仅剩顶项（表现为分数"钉在 0.5"、区分度丧失）。
        // 因此这里不再做绝对阈值截断，命中集由向量的绝对阈值与 top_k 共同约束。
        sorted.truncate(req.top_k);

        let final_hits = if req.mmr_enabled {
            let mut mmr_hits: Vec<MmrSearchHit> = sorted
                .into_iter()
                .map(|h| MmrSearchHit {
                    rerank_text: h.payload.content_preview.clone(),
                    inner: h,
                })
                .collect();

            // T3: 重排前批量取全文，覆盖 preview（provider 缺失时保持旧行为）。
            if let Some(provider) = &self.text_provider {
                let ids: Vec<Uuid> = mmr_hits.iter().map(|h| h.inner.memory_id).collect();
                if let Ok(texts) = provider.texts(&ids).await {
                    for hit in &mut mmr_hits {
                        if let Some(full) = texts.get(&hit.inner.memory_id) {
                            hit.rerank_text = full.clone();
                        }
                    }
                }
            }

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
        self.vector_store
            .add(id, embedding, payload.clone())
            .await?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SearchFilter, TantivyStore, VectorStore};

    /// 构造仅含向量召回的混合存储（FTS 文本与查询无词重叠，alpha=1.0）。
    fn vector_only_store() -> HybridStore {
        let vector_store = Arc::new(VectorStore::new(3));
        let fts_store = Arc::new(TantivyStore::new_test());
        HybridStore::new(
            vector_store,
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        )
    }

    /// 回归：向量侧归一化方向不得反转——最相似文档必须排第一。
    ///
    /// 此前 `merge_and_normalize` 对向量分做 `1.0 - 归一化`，令最相似项得 0 分垫底。
    #[tokio::test]
    async fn test_vector_similarity_keeps_best_first() {
        let hybrid = vector_only_store();
        let best = Uuid::new_v4();
        let mid = Uuid::new_v4();
        let worst = Uuid::new_v4();

        for (id, embedding) in [
            (best, vec![1.0_f32, 0.0, 0.0]),
            (mid, vec![0.9, 0.1, 0.0]),
            (worst, vec![0.0, 1.0, 0.0]),
        ] {
            hybrid
                .add(
                    &id,
                    &embedding,
                    "zzz_no_lexical_overlap",
                    VectorPayload {
                        content_preview: "zzz_no_lexical_overlap".to_string(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
        }
        hybrid.reload().unwrap();

        let req = HybridSearchRequest {
            query: "zzz_no_lexical_overlap".to_string(),
            query_embedding: vec![1.0, 0.0, 0.0],
            filter: SearchFilter {
                min_score: 0.0,
                ..Default::default()
            },
            top_k: 3,
            hybrid_alpha: 1.0,
            mmr_lambda: 0.5,
            mmr_enabled: false,
        };
        let result = hybrid.search(req).await.unwrap();
        let ids: Vec<Uuid> = result.hits.iter().map(|h| h.memory_id).collect();
        assert_eq!(
            ids.first(),
            Some(&best),
            "最相似文档应排第一，实际: {ids:?}"
        );
        assert_eq!(ids.last(), Some(&worst), "最不相似文档应排最后");
    }

    /// 回归：纯向量通路中，文档对查询自身的检索必须命中自己且排第一。
    #[tokio::test]
    async fn test_vector_self_retrieval_ranks_first() {
        let hybrid = vector_only_store();
        let target = Uuid::new_v4();
        hybrid
            .add(
                &target,
                &[1.0, 0.0, 0.0],
                "内容无关占位",
                VectorPayload {
                    content_preview: "内容无关占位".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        hybrid
            .add(
                &Uuid::new_v4(),
                &[0.0, 1.0, 0.0],
                "另一条无关占位",
                VectorPayload::default(),
            )
            .await
            .unwrap();
        hybrid.reload().unwrap();

        let req = HybridSearchRequest {
            query: "内容无关占位".to_string(),
            query_embedding: vec![1.0, 0.0, 0.0],
            filter: SearchFilter {
                min_score: 0.0,
                ..Default::default()
            },
            top_k: 2,
            hybrid_alpha: 1.0,
            mmr_lambda: 0.5,
            mmr_enabled: false,
        };
        let result = hybrid.search(req).await.unwrap();
        assert_eq!(result.hits.first().map(|h| h.memory_id), Some(target));
    }

    /// 回归：融合层不得再套用 `min_score` 绝对阈值。
    ///
    /// 纯向量命中（alpha=0.5）的顶项融合分恒为 0.5，恰等于默认 min_score；若仍按
    /// 绝对阈值截断，将只保留顶项、丢失其余已达原始相似度门槛的候选。修复后应保留
    /// 全部通过向量绝对阈值（余弦 >= min_score）的候选。
    #[tokio::test]
    async fn test_fused_min_score_does_not_truncate_to_top() {
        let vector_store = Arc::new(VectorStore::new(3));
        let fts_store = Arc::new(TantivyStore::new_test());
        let hybrid = HybridStore::new(
            vector_store,
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        );

        // 查询向量 [1,0,0]；三条余弦相似度 1.0 / 0.8 / 0.6，均 >= 0.5。
        // 存储文本与查询文本无词项重叠，避免 FTS 侧贡献干扰本用例。
        for (emb, text) in [
            (vec![1.0_f32, 0.0, 0.0], "stored_alpha_token"),
            (vec![0.8, 0.6, 0.0], "stored_beta_token"),
            (vec![0.6, 0.8, 0.0], "stored_gamma_token"),
        ] {
            hybrid
                .add(&Uuid::new_v4(), &emb, text, VectorPayload::default())
                .await
                .unwrap();
        }
        hybrid.reload().unwrap();

        let req = HybridSearchRequest {
            query: "query_delta_token".to_string(),
            query_embedding: vec![1.0, 0.0, 0.0],
            filter: SearchFilter {
                min_score: 0.5,
                ..Default::default()
            },
            top_k: 10,
            hybrid_alpha: 0.5,
            mmr_lambda: 0.5,
            mmr_enabled: false,
        };
        let result = hybrid.search(req).await.unwrap();
        assert_eq!(
            result.hits.len(),
            3,
            "融合层不应以 min_score 绝对阈值裁剪到仅剩顶项，实际: {:?}",
            result
                .hits
                .iter()
                .map(|h| (h.memory_id, h.score))
                .collect::<Vec<_>>()
        );
    }
}
