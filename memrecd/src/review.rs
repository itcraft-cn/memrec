//! # 写入后的异步冲突审查任务
//!
//! 写入路径完成“落库 + 规则去重”后，把新增记忆投递到本模块的队列；
//! 后台任务检索相似候选、调用 LLM 审查，并按结论标记失效或软删新记录。
//!
//! ## 执行约束
//!
//! - 顺序消费、单并发：避免同一记忆被并发审查导致重复标记；
//! - 审查只做标签/软删，不覆盖正文，全部可逆；
//! - 任何失败只计数并记录日志，不影响写入与检索。
//!
//! ## 与 Dream 的分工
//!
//! 本模块处理“单条新记忆 vs 既有记忆”的即时冲突；跨簇、跨主题的整合与
//! 矛盾消解仍由 Dream 的 Stage F/N 负责，两者互不替代。

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use mr_ability::dedup::{mark_superseded, ConflictReviewer, ReviewAction};
use mr_ability::embedding::EmbeddingGenerator;
use mr_ability::storage::{HybridStorage, MemoryStorage, MemoryStore, SearchFilter, VectorStorage};
use mr_common::{DedupConfig, SimilarMemory};

use crate::metrics::RetrievalMetrics;

/// 待审查的写入任务。
#[derive(Debug, Clone)]
pub struct AddReviewJob {
    /// 新增记忆 ID
    pub memory_id: Uuid,
    /// 新增记忆正文
    pub content: String,
    /// 所属项目（None 表示全局记忆）
    pub project_id: Option<Uuid>,
}

/// 后台审查循环：顺序消费审查任务，直到通道关闭。
#[allow(clippy::too_many_arguments)]
pub async fn review_loop(
    storage: Arc<MemoryStore>,
    vector_store: Arc<dyn VectorStorage>,
    embedder: Arc<dyn EmbeddingGenerator>,
    hybrid_store: Arc<dyn HybridStorage>,
    reviewer: Arc<dyn ConflictReviewer>,
    metrics: Arc<RetrievalMetrics>,
    config: DedupConfig,
    mut receiver: mpsc::Receiver<AddReviewJob>,
) {
    info!(
        "Conflict review loop started: min_score={}, max_candidates={}",
        config.llm_review_min_score, config.llm_review_max_candidates
    );

    while let Some(job) = receiver.recv().await {
        match process_job(
            &*storage,
            &*vector_store,
            &*embedder,
            &*hybrid_store,
            &*reviewer,
            &config,
            &job,
        )
        .await
        {
            Ok(ReviewAction::Keep) => metrics.on_review_keep(),
            Ok(ReviewAction::SupersedeNew(target)) => {
                info!(
                    target = %target,
                    new = %job.memory_id,
                    "Conflict review: superseded new memory"
                );
                metrics.on_review_supersede_new();
            }
            Ok(ReviewAction::SupersedeOld(target)) => {
                info!(
                    target = %target,
                    new = %job.memory_id,
                    "Conflict review: superseded old memory"
                );
                metrics.on_review_supersede_old();
            }
            Err(e) => {
                warn!("Conflict review failed for {}: {:#}", job.memory_id, e);
                metrics.on_review_failed();
            }
        }
    }
}

/// 处理单条审查任务。
async fn process_job(
    storage: &dyn MemoryStorage,
    vector_store: &dyn VectorStorage,
    embedder: &dyn EmbeddingGenerator,
    hybrid_store: &dyn HybridStorage,
    reviewer: &dyn ConflictReviewer,
    config: &DedupConfig,
    job: &AddReviewJob,
) -> anyhow::Result<ReviewAction> {
    // 记忆可能已被用户删除：静默跳过
    let Some(memory) = storage.get(&job.memory_id).await? else {
        return Ok(ReviewAction::Keep);
    };
    if memory.is_deleted {
        return Ok(ReviewAction::Keep);
    }

    let embedding = embedder.embed(&job.content)?;
    let filter = SearchFilter {
        project_id: job.project_id,
        include_global: true,
        min_score: config.llm_review_min_score,
        ..Default::default()
    };
    let hits = vector_store
        .search(&embedding, filter, config.llm_review_max_candidates.max(1))
        .await?;

    let mut candidates: Vec<SimilarMemory> = Vec::new();
    for hit in hits {
        if hit.memory_id == job.memory_id || hit.score < config.llm_review_min_score {
            continue;
        }
        // 向量载荷只有 200 字预览；审查需要完整语义，回表取正文并限长。
        let content = match storage.get(&hit.memory_id).await? {
            Some(m) if !m.is_deleted => m.content.chars().take(800).collect(),
            _ => continue,
        };
        candidates.push(SimilarMemory {
            memory_id: hit.memory_id,
            content,
            score: hit.score,
        });
    }

    if candidates.is_empty() {
        debug!("Conflict review: no candidates for {}", job.memory_id);
        return Ok(ReviewAction::Keep);
    }

    let decision = reviewer.review(&job.content, &candidates).await;
    debug!(
        "Conflict review {}: {:?} ({})",
        job.memory_id, decision.action, decision.reason
    );

    match decision.action {
        ReviewAction::Keep => Ok(ReviewAction::Keep),
        ReviewAction::SupersedeNew(target) => {
            if target == job.memory_id {
                return Ok(ReviewAction::Keep);
            }
            // 先标记再软删：即使删除失败，标签仍可审计；软删可 restore 撤销。
            mark_superseded(storage, &job.memory_id, &target, &decision.reason).await?;
            storage.delete(&job.memory_id).await?;
            let _ = hybrid_store.remove(&job.memory_id).await;
            Ok(ReviewAction::SupersedeNew(target))
        }
        ReviewAction::SupersedeOld(target) => {
            if target == job.memory_id {
                return Ok(ReviewAction::Keep);
            }
            mark_superseded(storage, &target, &job.memory_id, &decision.reason).await?;
            Ok(ReviewAction::SupersedeOld(target))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mr_ability::dedup::ReviewDecision;
    use mr_ability::storage::{
        HybridSearchRequest, HybridSearchResult, RocksDBStore, VectorPayload, VectorStore,
    };
    use mr_common::{Memory, MemoryType};

    /// 固定向量：所有文本映射到同一向量，保证相似度检索必命中。
    struct FixedEmbedder;

    impl EmbeddingGenerator for FixedEmbedder {
        fn dimension(&self) -> usize {
            4
        }
        fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        }
        fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }
    }

    /// 只记录动作的审查器。
    struct FixedReviewer(ReviewDecision);

    #[async_trait]
    impl ConflictReviewer for FixedReviewer {
        async fn review(&self, _content: &str, _candidates: &[SimilarMemory]) -> ReviewDecision {
            self.0.clone()
        }
    }

    struct FakeHybrid;

    #[async_trait]
    impl HybridStorage for FakeHybrid {
        async fn search(&self, _req: HybridSearchRequest) -> anyhow::Result<HybridSearchResult> {
            Ok(HybridSearchResult {
                hits: Vec::new(),
                vec_count: 0,
                fts_count: 0,
            })
        }
        async fn add(
            &self,
            _id: &Uuid,
            _embedding: &[f32],
            _text: &str,
            _payload: VectorPayload,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove(&self, _id: &Uuid) -> anyhow::Result<bool> {
            Ok(true)
        }
    }

    struct Env {
        _dir: tempfile::TempDir,
        storage: Arc<MemoryStore>,
        vectors: Arc<VectorStore>,
        embedder: Arc<FixedEmbedder>,
        project: Uuid,
    }

    impl Env {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let rocksdb = Arc::new(RocksDBStore::open(dir.path()).unwrap());
            Self {
                _dir: dir,
                storage: Arc::new(MemoryStore::new(rocksdb)),
                vectors: Arc::new(VectorStore::new(4)),
                embedder: Arc::new(FixedEmbedder),
                project: Uuid::new_v4(),
            }
        }

        async fn add_memory(&self, content: &str) -> Memory {
            let m =
                Memory::new(content.to_string(), MemoryType::Knowledge).with_project(self.project);
            self.storage.save(&m).await.unwrap();
            let embedding = self.embedder.embed(content).unwrap();
            self.vectors
                .add(
                    &m.id,
                    &embedding,
                    VectorPayload {
                        project_id: m.project_id,
                        memory_type: m.memory_type.to_string(),
                        tags: m.tags.clone(),
                        content_preview: content.chars().take(200).collect(),
                        importance: m.importance,
                        chunk_group_id: None,
                        chunk_index: None,
                        chunk_total: None,
                    },
                )
                .await
                .unwrap();
            m
        }
    }

    fn config() -> DedupConfig {
        DedupConfig {
            llm_review_min_score: 0.5,
            llm_review_max_candidates: 5,
            ..DedupConfig::default()
        }
    }

    async fn run(env: &Env, job: &AddReviewJob, action: ReviewAction) -> ReviewAction {
        process_job(
            &*env.storage,
            &*env.vectors,
            &*env.embedder,
            &FakeHybrid,
            &FixedReviewer(ReviewDecision {
                action: action.clone(),
                reason: "test".to_string(),
            }),
            &config(),
            job,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_review_supersede_old_marks_target() {
        let env = Env::new();
        let old = env.add_memory("旧结论：缓存用 Redis").await;
        let new = env.add_memory("新结论：缓存用 RocksDB").await;

        let action = run(
            &env,
            &AddReviewJob {
                memory_id: new.id,
                content: new.content.clone(),
                project_id: new.project_id,
            },
            ReviewAction::SupersedeOld(old.id),
        )
        .await;
        assert_eq!(action, ReviewAction::SupersedeOld(old.id));

        let old_after = env.storage.get(&old.id).await.unwrap().unwrap();
        assert!(old_after.tags.iter().any(|t| t == "superseded"));
        assert_eq!(
            old_after.metadata.get("superseded_by"),
            Some(&new.id.to_string())
        );
        assert!(!old_after.is_deleted, "旧记忆只标记失效，不软删");

        let new_after = env.storage.get(&new.id).await.unwrap().unwrap();
        assert!(!new_after.is_deleted, "新记忆应保留");
    }

    #[tokio::test]
    async fn test_review_supersede_new_soft_deletes_new() {
        let env = Env::new();
        let old = env.add_memory("既有事实").await;
        let new = env.add_memory("既有事实（重复）").await;

        let action = run(
            &env,
            &AddReviewJob {
                memory_id: new.id,
                content: new.content.clone(),
                project_id: new.project_id,
            },
            ReviewAction::SupersedeNew(old.id),
        )
        .await;
        assert_eq!(action, ReviewAction::SupersedeNew(old.id));

        let new_after = env.storage.get(&new.id).await.unwrap().unwrap();
        assert!(new_after.is_deleted, "重复的新记忆应软删（可 restore）");
        assert!(new_after.tags.iter().any(|t| t == "superseded"));
        assert_eq!(
            new_after.metadata.get("superseded_by"),
            Some(&old.id.to_string())
        );

        let old_after = env.storage.get(&old.id).await.unwrap().unwrap();
        assert!(!old_after.is_deleted);
    }

    #[tokio::test]
    async fn test_review_keep_changes_nothing() {
        let env = Env::new();
        let old = env.add_memory("事实 A").await;
        let new = env.add_memory("事实 B").await;

        let action = run(
            &env,
            &AddReviewJob {
                memory_id: new.id,
                content: new.content.clone(),
                project_id: new.project_id,
            },
            ReviewAction::Keep,
        )
        .await;
        assert_eq!(action, ReviewAction::Keep);

        assert!(!env.storage.get(&new.id).await.unwrap().unwrap().is_deleted);
        assert!(!env.storage.get(&old.id).await.unwrap().unwrap().is_deleted);
    }

    #[tokio::test]
    async fn test_review_skips_deleted_new_memory() {
        let env = Env::new();
        let new = env.add_memory("将被删除").await;
        env.storage.delete(&new.id).await.unwrap();

        let action = run(
            &env,
            &AddReviewJob {
                memory_id: new.id,
                content: new.content.clone(),
                project_id: new.project_id,
            },
            ReviewAction::SupersedeOld(Uuid::new_v4()),
        )
        .await;
        assert_eq!(action, ReviewAction::Keep);
    }
}
