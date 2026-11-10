//! # 周期性冲突审查任务
//!
//! 写入路径完成“落库 + 规则去重”后不再即时调用 LLM；改为按 **cron 表达式**
//! 在低峰时段批量审查上一周期新增的记忆：检索相似候选、判断重复/矛盾，并按结论
//! 标记失效或软删新记录。
//!
//! ## 调度语义
//!
//! - 默认表达式 `0 4 * * 6`（周六 04:00，本地时间），落在 DeepSeek 空闲计费时段；
//! - 错过窗口不补跑：守护进程停机跨过该时刻时，下一次按新周期计算；
//! - 单次处理条数由 `llm_review_max_per_run` 限制（普通数值阈值；设为超大值即全量，
//!   设为 0 表示本轮不处理）；超出部分直接丢弃，不滚入下一周；
//! - 上次执行时间持久化到 `~/.memrec/review.state`，用于确定审查窗口。
//!
//! ## 候选阈值口径
//!
//! | 口径 | 判定 |
//! |------|------|
//! | `absolute` | `score >= min_score` |
//! | `ratio` | `score >= best_score * score_ratio` |
//! | `hybrid` | `score >= max(min_score, best_score * score_ratio)` |
//!
//! ## 执行约束
//!
//! - 顺序消费、单并发：避免同一记忆被并发审查导致重复标记；
//! - 审查只做标签/软删，不覆盖正文，全部可逆；
//! - 任何失败只计数并记录日志，不影响写入与检索。
//!
//! ## 与 Dream 的分工
//!
//! 本模块处理“单条新记忆 vs 既有记忆”的冲突；跨簇、跨主题的整合与矛盾消解
//! 仍由 Dream 的 Stage F/N 负责，两者互不替代。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Local, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use mr_ability::dedup::{mark_superseded, ConflictReviewer, ReviewAction};
use mr_ability::embedding::EmbeddingGenerator;
use mr_ability::storage::{HybridStorage, MemoryStorage, MemoryStore, SearchFilter, VectorStorage};
use mr_common::{CandidateThresholdMode, DedupConfig, Memory, MemorySource, SimilarMemory};

use crate::cron::CronSchedule;
use crate::metrics::RetrievalMetrics;

/// 审查任务的持久化状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewState {
    /// 上次审查完成时间（Unix 秒）
    pub last_run_at: i64,
}

/// 读取上次审查时间；文件缺失或损坏时返回 None（按 7 天窗口处理）。
pub fn load_state(path: &Path) -> Option<ReviewState> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 写入上次审查时间（失败只告警，不影响本轮结果）。
pub fn save_state(path: &Path, last_run_at: i64) {
    let state = ReviewState { last_run_at };
    match serde_json::to_string_pretty(&state) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                warn!("Failed to write review state {}: {}", path.display(), e);
            }
        }
        Err(e) => warn!("Failed to serialize review state: {}", e),
    }
}

/// 一次审查的执行结果（用于日志与测试）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReviewRunSummary {
    /// 实际送审条数
    pub selected: usize,
    /// 因单次上限被丢弃的条数
    pub skipped: usize,
    pub keep: usize,
    pub supersede_new: usize,
    pub supersede_old: usize,
    pub failed: usize,
}

/// 审查主循环：按 cron 计算下一次执行时刻，睡到点后跑一轮，再进入下一周期。
#[allow(clippy::too_many_arguments)]
pub async fn review_loop(
    storage: Arc<MemoryStore>,
    vector_store: Arc<dyn VectorStorage>,
    embedder: Arc<dyn EmbeddingGenerator>,
    hybrid_store: Arc<dyn HybridStorage>,
    reviewer: Arc<dyn ConflictReviewer>,
    metrics: Arc<RetrievalMetrics>,
    config: DedupConfig,
    state_path: PathBuf,
) {
    let schedule = match CronSchedule::parse(&config.llm_review_cron) {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "Invalid llm_review_cron={:?} ({}), falling back to \"0 4 * * 6\"",
                config.llm_review_cron, e
            );
            CronSchedule::parse("0 4 * * 6").expect("default cron must parse")
        }
    };

    info!(
        "Conflict review scheduler started: cron=\"{}\", min_score={}, mode={}, score_ratio={}, max_candidates={}, max_per_run={}",
        config.llm_review_cron,
        config.llm_review_min_score,
        config.llm_review_threshold_mode,
        config.llm_review_score_ratio,
        config.llm_review_max_candidates,
        config.llm_review_max_per_run
    );

    loop {
        let Some(next) = schedule.next_after(Local::now()) else {
            warn!(
                "No next run within 400 days for cron \"{}\"; review disabled",
                config.llm_review_cron
            );
            return;
        };
        let wait = (next - Local::now())
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(60));
        info!(
            "Next conflict review at {}",
            next.format("%Y-%m-%d %H:%M %:z")
        );
        tokio::time::sleep(wait).await;

        match run_once(
            &*storage,
            &*vector_store,
            &*embedder,
            &*hybrid_store,
            &*reviewer,
            &metrics,
            &config,
            &state_path,
        )
        .await
        {
            Ok(summary) => info!(
                "Conflict review finished: selected={} skipped={} keep={} supersede_new={} supersede_old={} failed={}",
                summary.selected,
                summary.skipped,
                summary.keep,
                summary.supersede_new,
                summary.supersede_old,
                summary.failed
            ),
            Err(e) => warn!("Conflict review run failed: {:#}", e),
        }
    }
}

/// 执行一轮审查。
///
/// 审查窗口为 `(上次执行时间, 现在]`；无状态文件时回退为最近 7 天。
/// 无论本轮是否全部成功，完成后都会推进 `last_run_at`（漏跑即丢弃，不补跑）。
#[allow(clippy::too_many_arguments)]
pub async fn run_once(
    storage: &dyn MemoryStorage,
    vector_store: &dyn VectorStorage,
    embedder: &dyn EmbeddingGenerator,
    hybrid_store: &dyn HybridStorage,
    reviewer: &dyn ConflictReviewer,
    metrics: &RetrievalMetrics,
    config: &DedupConfig,
    state_path: &Path,
) -> Result<ReviewRunSummary> {
    let now = Utc::now();
    let window_start = load_state(state_path)
        .map(|s| s.last_run_at)
        .unwrap_or_else(|| (now - ChronoDuration::days(7)).timestamp());

    // 区间按秒精度且不含上界，+1 秒保证“本轮开始前刚写入”的记忆也能进入窗口。
    let window_end = now.timestamp() + 1;
    let mut candidates = storage
        .list_created_between(window_start, window_end, usize::MAX)
        .await
        .with_context(|| format!("failed to load review window ({window_start}, {window_end})"))?;

    candidates.retain(is_reviewable);
    // 优先审查最近写入的记忆：上限截断时保留信息更新鲜的一批。
    candidates.sort_by_key(|m| std::cmp::Reverse(m.created_at));

    let cap = config.llm_review_max_per_run;
    let skipped = candidates.len().saturating_sub(cap);
    candidates.truncate(cap);

    let mut summary = ReviewRunSummary {
        selected: candidates.len(),
        skipped,
        ..Default::default()
    };

    for memory in &candidates {
        match process_job(
            storage,
            vector_store,
            embedder,
            hybrid_store,
            reviewer,
            config,
            memory,
        )
        .await
        {
            Ok(ReviewAction::Keep) => {
                summary.keep += 1;
                metrics.on_review_keep();
            }
            Ok(ReviewAction::SupersedeNew(target)) => {
                summary.supersede_new += 1;
                metrics.on_review_supersede_new();
                info!(target = %target, new = %memory.id, "Conflict review: superseded new memory");
            }
            Ok(ReviewAction::SupersedeOld(target)) => {
                summary.supersede_old += 1;
                metrics.on_review_supersede_old();
                info!(target = %target, new = %memory.id, "Conflict review: superseded old memory");
            }
            Err(e) => {
                warn!("Conflict review failed for {}: {:#}", memory.id, e);
                summary.failed += 1;
                metrics.on_review_failed();
            }
        }
    }

    metrics.on_review_run(
        now.timestamp(),
        summary.selected as u64,
        summary.skipped as u64,
    );
    save_state(state_path, now.timestamp());
    Ok(summary)
}

/// 是否参与审查：跳过软删、Dream 产出与分块写入。
fn is_reviewable(memory: &Memory) -> bool {
    !memory.is_deleted
        && memory.source != MemorySource::System
        && !memory.tags.iter().any(|t| t.starts_with("part:"))
}

/// 计算候选筛选下限。
///
/// `best` 为除自身外的最高相似度；无候选时返回 `None`。
pub fn candidate_floor(
    mode: CandidateThresholdMode,
    min_score: f32,
    score_ratio: f32,
    best: f32,
) -> Option<f32> {
    if !best.is_finite() || best <= 0.0 {
        return None;
    }
    let ratio = score_ratio.clamp(0.0, 1.0);
    let floor = match mode {
        CandidateThresholdMode::Absolute => min_score,
        CandidateThresholdMode::Ratio => best * ratio,
        CandidateThresholdMode::Hybrid => min_score.max(best * ratio),
    };
    Some(floor.clamp(0.0, 1.0))
}

/// 处理单条记忆：检索候选 → LLM 审查 → 落地动作。
async fn process_job(
    storage: &dyn MemoryStorage,
    vector_store: &dyn VectorStorage,
    embedder: &dyn EmbeddingGenerator,
    hybrid_store: &dyn HybridStorage,
    reviewer: &dyn ConflictReviewer,
    config: &DedupConfig,
    memory: &Memory,
) -> Result<ReviewAction> {
    // 记忆可能已被用户删除：静默跳过
    if memory.is_deleted {
        return Ok(ReviewAction::Keep);
    }

    let embedding = embedder.embed(&memory.content)?;
    // 比例口径需要先拿到最佳候选分，因此检索阶段不按绝对阈值预过滤。
    let search_min_score = match config.llm_review_threshold_mode {
        CandidateThresholdMode::Ratio => 0.0,
        _ => config.llm_review_min_score.clamp(0.0, 1.0),
    };
    let filter = SearchFilter {
        project_id: memory.project_id,
        include_global: true,
        min_score: search_min_score,
        ..Default::default()
    };
    let hits = vector_store
        .search(&embedding, filter, config.llm_review_max_candidates.max(1))
        .await?;

    let peer_best = hits
        .iter()
        .filter(|h| h.memory_id != memory.id)
        .map(|h| h.score)
        .fold(0.0f32, f32::max);
    let Some(floor) = candidate_floor(
        config.llm_review_threshold_mode,
        config.llm_review_min_score,
        config.llm_review_score_ratio,
        peer_best,
    ) else {
        debug!("Conflict review: no candidates for {}", memory.id);
        return Ok(ReviewAction::Keep);
    };

    let mut candidates: Vec<SimilarMemory> = Vec::new();
    for hit in hits {
        if hit.memory_id == memory.id || hit.score < floor {
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
        if candidates.len() >= config.llm_review_max_candidates.max(1) {
            break;
        }
    }

    if candidates.is_empty() {
        debug!("Conflict review: no candidates for {}", memory.id);
        return Ok(ReviewAction::Keep);
    }

    let decision = reviewer.review(&memory.content, &candidates).await;
    debug!(
        "Conflict review {}: {:?} ({})",
        memory.id, decision.action, decision.reason
    );

    match decision.action {
        ReviewAction::Keep => Ok(ReviewAction::Keep),
        ReviewAction::SupersedeNew(target) => {
            if target == memory.id {
                return Ok(ReviewAction::Keep);
            }
            // 先标记再软删：即使删除失败，标签仍可审计；软删可 restore 撤销。
            mark_superseded(storage, &memory.id, &target, &decision.reason).await?;
            storage.delete(&memory.id).await?;
            let _ = hybrid_store.remove(&memory.id).await;
            Ok(ReviewAction::SupersedeNew(target))
        }
        ReviewAction::SupersedeOld(target) => {
            if target == memory.id {
                return Ok(ReviewAction::Keep);
            }
            mark_superseded(storage, &target, &memory.id, &decision.reason).await?;
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
    use uuid::Uuid;

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

    /// 固定结论的审查器。
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
            llm_review_max_per_run: 50,
            ..DedupConfig::default()
        }
    }

    async fn review_memory(env: &Env, memory: &Memory, action: ReviewAction) -> ReviewAction {
        process_job(
            &*env.storage,
            &*env.vectors,
            &*env.embedder,
            &FakeHybrid,
            &FixedReviewer(ReviewDecision {
                action,
                reason: "test".to_string(),
            }),
            &config(),
            memory,
        )
        .await
        .unwrap()
    }

    #[test]
    fn test_candidate_floor_modes() {
        // absolute：直接用下限
        assert_eq!(
            candidate_floor(CandidateThresholdMode::Absolute, 0.7, 0.9, 0.95),
            Some(0.7)
        );
        // ratio：最佳分 × 比例
        let ratio = candidate_floor(CandidateThresholdMode::Ratio, 0.7, 0.9, 0.95).unwrap();
        assert!((ratio - 0.855).abs() < 1e-6);
        // hybrid：两者取严
        let hybrid = candidate_floor(CandidateThresholdMode::Hybrid, 0.7, 0.9, 0.95).unwrap();
        assert!((hybrid - 0.855).abs() < 1e-6);
        let hybrid = candidate_floor(CandidateThresholdMode::Hybrid, 0.7, 0.9, 0.72).unwrap();
        assert!((hybrid - 0.7).abs() < 1e-6);
        // 无候选
        assert_eq!(
            candidate_floor(CandidateThresholdMode::Ratio, 0.7, 0.9, 0.0),
            None
        );
    }

    #[tokio::test]
    async fn test_review_supersede_old_marks_target() {
        let env = Env::new();
        let old = env.add_memory("旧结论：缓存用 Redis").await;
        let new = env.add_memory("新结论：缓存用 RocksDB").await;

        let action = review_memory(&env, &new, ReviewAction::SupersedeOld(old.id)).await;
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

        let action = review_memory(&env, &new, ReviewAction::SupersedeNew(old.id)).await;
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

        let action = review_memory(&env, &new, ReviewAction::Keep).await;
        assert_eq!(action, ReviewAction::Keep);
        assert!(!env.storage.get(&new.id).await.unwrap().unwrap().is_deleted);
        assert!(!env.storage.get(&old.id).await.unwrap().unwrap().is_deleted);
    }

    #[tokio::test]
    async fn test_run_once_respects_cap_and_filters() {
        let env = Env::new();
        // 4 条可审记忆 + 1 条 System（Dream 产出）+ 1 条分块写入 + 1 条软删
        for i in 0..4 {
            env.add_memory(&format!("待审记忆 {i}")).await;
        }
        let mut system = env.add_memory("dream output").await;
        system.source = MemorySource::System;
        env.storage.update(&system).await.unwrap();

        let mut chunk = env.add_memory("chunked part").await;
        chunk.tags.push("part:1-2".to_string());
        env.storage.update(&chunk).await.unwrap();

        let deleted = env.add_memory("deleted memory").await;
        env.storage.delete(&deleted.id).await.unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let state_path = state_dir.path().join("review.state");
        let metrics = RetrievalMetrics::new();
        let mut cfg = config();
        cfg.llm_review_max_per_run = 2;

        let summary = run_once(
            &*env.storage,
            &*env.vectors,
            &*env.embedder,
            &FakeHybrid,
            &FixedReviewer(ReviewDecision::keep("test")),
            &metrics,
            &cfg,
            &state_path,
        )
        .await
        .unwrap();

        assert_eq!(summary.selected, 2, "只处理上限内的记忆");
        assert_eq!(summary.skipped, 2, "其余可审记忆因上限被丢弃");
        assert_eq!(summary.keep, 2);
        assert_eq!(summary.failed, 0);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.review_runs, 1);
        assert_eq!(snapshot.review_queued, 2);
        assert_eq!(snapshot.review_skipped, 2);
        assert!(snapshot.review_last_run_at.is_some());

        let state = load_state(&state_path).expect("本轮结束应落状态");
        assert!(state.last_run_at > 0);
    }

    #[tokio::test]
    async fn test_run_once_zero_cap_processes_nothing() {
        let env = Env::new();
        env.add_memory("待审记忆").await;

        let state_dir = tempfile::tempdir().unwrap();
        let state_path = state_dir.path().join("review.state");
        let mut cfg = config();
        cfg.llm_review_max_per_run = 0;

        let summary = run_once(
            &*env.storage,
            &*env.vectors,
            &*env.embedder,
            &FakeHybrid,
            &FixedReviewer(ReviewDecision::keep("test")),
            &RetrievalMetrics::new(),
            &cfg,
            &state_path,
        )
        .await
        .unwrap();

        assert_eq!(summary.selected, 0);
        assert_eq!(summary.skipped, 1);
    }
}
