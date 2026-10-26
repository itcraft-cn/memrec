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

/// RRF 平滑常数：越小越强调头部名次。60 为常用默认值。
const RRF_K: f32 = 60.0;

/// 全文单路命中的最低查询词覆盖率。
///
/// 低于该值时视为"个别字词碰撞"而非真实相关，避免单路 BM25 命中凭一个词
/// 获得与语义命中相当的相关度。
const MIN_FTS_COVERAGE: f32 = 0.30;

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

    /// 合并向量与全文检索结果，生成可解释的校准相关度。
    ///
    /// ## 分数模型
    ///
    /// - 向量贡献：原始余弦 `cos`（绝对值，0-1）。
    /// - 全文贡献：查询词覆盖率 `coverage`（命中查询词数 / 查询词总数），
    ///   而不是 BM25 原始分。BM25 量纲随查询与语料漂移，无法作为绝对阈值；
    ///   覆盖率有明确语义，且能识别"只撞上一个词"的弱命中。
    /// - 双路命中：`1 - (1 - cos)(1 - coverage)`（noisy-OR），两路都强时相互增强，
    ///   单路命中时退化为该路自身的绝对分。
    /// - `rank_score`：加权 RRF（`alpha` 向量路 + `1-alpha` 全文路），仅用于同分裁决
    ///   与可解释性，不再把单路顶项固定为 `alpha`/`1-alpha`。
    ///
    /// 全文单路且覆盖率低于 [`MIN_FTS_COVERAGE`] 的候选在此直接丢弃；融合后的
    /// 绝对阈值由调用方按双路证据统一判定。
    fn merge_and_calibrate(
        vec_hits: Vec<SearchHit>,
        fts_hits: Vec<SearchHit>,
        alpha: f32,
    ) -> Vec<SearchHit> {
        if vec_hits.is_empty() && fts_hits.is_empty() {
            return Vec::new();
        }
        let alpha = alpha.clamp(0.0, 1.0);

        // 名次映射：原始分降序，同分按 id 升序，保证多次运行结果确定。
        fn rank_map(hits: &[SearchHit], raw: impl Fn(&SearchHit) -> f32) -> HashMap<Uuid, usize> {
            let mut order: Vec<(Uuid, f32)> = hits.iter().map(|h| (h.memory_id, raw(h))).collect();
            order.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });
            order
                .into_iter()
                .enumerate()
                .map(|(i, (id, _))| (id, i + 1))
                .collect()
        }

        let vec_rank = rank_map(&vec_hits, |h| h.vector_score.unwrap_or(h.score));
        let fts_rank = rank_map(&fts_hits, |h| h.fts_score.unwrap_or(h.score));

        let mut merged: HashMap<Uuid, SearchHit> = HashMap::new();

        for hit in vec_hits {
            // 向量存储返回的 score 即原始余弦；这里直接作为绝对相关度。
            let vec_abs = hit.score.clamp(0.0, 1.0);
            let rank = vec_rank.get(&hit.memory_id).copied().unwrap_or(1);
            let rrf = alpha / (RRF_K + rank as f32);
            let mut hit = hit;
            hit.score = vec_abs;
            hit.vector_score = hit.vector_score.or(Some(vec_abs));
            hit.fts_score = None;
            hit.coverage = None;
            hit.rank_score = Some(rrf);
            merged.insert(hit.memory_id, hit);
        }

        for hit in fts_hits {
            let coverage = hit.coverage.unwrap_or(0.0).clamp(0.0, 1.0);
            let fts_raw = hit.fts_score.unwrap_or(hit.score);

            if let Some(existing) = merged.get_mut(&hit.memory_id) {
                // 双路命中：保留向量载荷（含 chunk 信息），补全全文侧字段并做 noisy-OR。
                let vec_abs = existing
                    .vector_score
                    .unwrap_or(existing.score)
                    .clamp(0.0, 1.0);
                existing.score = 1.0 - (1.0 - vec_abs) * (1.0 - coverage);
                existing.fts_score = Some(fts_raw);
                existing.coverage = Some(coverage);
                let rank = fts_rank.get(&hit.memory_id).copied().unwrap_or(1);
                let rrf = (1.0 - alpha) / (RRF_K + rank as f32);
                existing.rank_score = Some(existing.rank_score.unwrap_or(0.0) + rrf);
            } else {
                // 全文单路：低覆盖视为字词碰撞，直接丢弃。
                if coverage < MIN_FTS_COVERAGE {
                    continue;
                }
                let rank = fts_rank.get(&hit.memory_id).copied().unwrap_or(1);
                let rrf = (1.0 - alpha) / (RRF_K + rank as f32);
                let mut hit = hit;
                hit.score = coverage;
                hit.vector_score = None;
                hit.fts_score = Some(fts_raw);
                hit.coverage = Some(coverage);
                hit.rank_score = Some(rrf);
                merged.insert(hit.memory_id, hit);
            }
        }

        let mut result: Vec<SearchHit> = merged.into_values().collect();
        // 确定性排序：相关度降序 -> RRF 名次分降序 -> memory_id 升序。
        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.rank_score
                        .unwrap_or(0.0)
                        .partial_cmp(&a.rank_score.unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
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
        // 向量侧不在 KNN 阶段应用 min_score：融合需要同时看到原始余弦与查询词覆盖率，
        // 过早裁掉会让"低余弦但高词覆盖"的精确 token 命中无法进入融合。阈值在融合后
        // 按双路证据统一判定（见下方 retain）。
        let mut vector_filter = filter.clone();
        vector_filter.min_score = 0.0;
        let (vec_result, fts_result) = tokio::join!(
            self.vector_store
                .search(&req.query_embedding, vector_filter, req.top_k),
            self.fts_store.search(&req.query, filter, req.top_k)
        );

        let vec_hits = vec_result?;
        let fts_hits = fts_result?;

        let vec_count = vec_hits.len();
        let fts_count = fts_hits.len();

        let merged = Self::merge_and_calibrate(vec_hits, fts_hits, req.hybrid_alpha);

        // 3. Apply scoring (time decay + source weight)
        // TODO: Need created_at/scope/source in VectorPayload or SearchHit for scoring
        // For now, skip scoring step - scorer_config is reserved for future use
        let _ = &self.scorer_config;

        // 融合后按"绝对证据"统一过滤：
        // - 向量证据：原始余弦 >= min_score；
        // - 全文证据：查询词覆盖率 >= MIN_FTS_COVERAGE（单路低覆盖视为字词碰撞）；
        // - 至少一路成立，且 noisy-OR 融合分 >= min_score。
        // 这样既不会把"单路 BM25 命中"直接抬到 0.5，也不会让两路都很弱的候选
        // 靠 noisy-OR 互相抬轿子。
        let min_score = req.filter.min_score;
        let mut sorted: Vec<SearchHit> = merged
            .into_iter()
            .filter(|h| {
                let vec_ok = h.vector_score.is_some_and(|v| v >= min_score);
                let fts_ok = h.coverage.is_some_and(|c| c >= MIN_FTS_COVERAGE);
                (vec_ok || fts_ok) && h.score >= min_score
            })
            .collect();
        sorted.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.rank_score
                        .unwrap_or(0.0)
                        .partial_cmp(&a.rank_score.unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.memory_id.cmp(&b.memory_id))
        });
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

    /// 回归：融合层以绝对校准分排序，`min_score` 作用在绝对相关度上。
    ///
    /// 三条候选余弦为 1.0 / 0.8 / 0.6，均 >= 0.5，故全部保留；不再出现
    /// "纯向量顶项恒为 alpha=0.5、恰等于阈值"的裁剪现象。
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

    /// 构造内部命中，便于直接验证融合函数。
    fn make_hit(
        id: Uuid,
        score: f32,
        vector_score: Option<f32>,
        fts_score: Option<f32>,
        coverage: Option<f32>,
    ) -> SearchHit {
        SearchHit {
            memory_id: id,
            score,
            payload: VectorPayload::default(),
            vector_score,
            fts_score,
            coverage,
            rank_score: None,
        }
    }

    /// 单路向量命中不得被固定为 alpha（默认 0.5），必须保留原始余弦的绝对量纲。
    #[test]
    fn test_merge_calibrate_vector_score_is_absolute() {
        let id = Uuid::new_v4();
        let out = HybridStore::merge_and_calibrate(
            vec![make_hit(id, 0.58, Some(0.58), None, None)],
            vec![],
            0.5,
        );
        assert_eq!(out.len(), 1);
        assert!(
            (out[0].score - 0.58).abs() < 1e-6,
            "向量单路应保留绝对余弦，实际 {}",
            out[0].score
        );
    }

    /// 全文单路且覆盖率过低：视为字词碰撞，融合层直接丢弃。
    #[test]
    fn test_merge_calibrate_drops_low_coverage_fts_only() {
        let id = Uuid::new_v4();
        let out = HybridStore::merge_and_calibrate(
            vec![],
            vec![make_hit(id, 0.0, None, Some(3.0), Some(0.17))],
            0.5,
        );
        assert!(out.is_empty(), "低覆盖全文命中应被丢弃: {out:?}");
    }

    /// 双路命中按 noisy-OR 合成：单路都强时相关度接近 1，且保留两路原始分。
    #[test]
    fn test_merge_calibrate_dual_route_noisy_or() {
        let id = Uuid::new_v4();
        let out = HybridStore::merge_and_calibrate(
            vec![make_hit(id, 0.60, Some(0.60), None, None)],
            vec![make_hit(id, 0.0, None, Some(4.0), Some(1.0))],
            0.5,
        );
        assert_eq!(out.len(), 1);
        assert!(
            (out[0].score - 1.0).abs() < 1e-6,
            "双路满分应合成 1.0，实际 {}",
            out[0].score
        );
        assert_eq!(out[0].vector_score, Some(0.60));
        assert_eq!(out[0].fts_score, Some(4.0));
        assert_eq!(out[0].coverage, Some(1.0));
        assert!(out[0].rank_score.unwrap_or(0.0) > 0.0);
    }

    /// 端到端回归：无关查询（仅个别字词命中）不得返回结果。
    #[tokio::test]
    async fn test_unrelated_query_returns_empty_with_default_threshold() {
        let hybrid = vector_only_store();
        // 与查询向量正交（cos=0），仅"无关"一词与查询重叠。
        hybrid
            .add(
                &Uuid::new_v4(),
                &[0.0, 1.0, 0.0],
                "无关模块：panel 面板随机效应",
                VectorPayload {
                    content_preview: "无关模块：panel 面板随机效应".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        hybrid.reload().unwrap();

        let req = HybridSearchRequest {
            query: "完全无关的烘焙面包配方".to_string(),
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
        assert!(
            result.hits.is_empty(),
            "无关查询不应返回结果，实际: {:?}",
            result
                .hits
                .iter()
                .map(|h| (h.memory_id, h.score, h.coverage))
                .collect::<Vec<_>>()
        );
    }
}
