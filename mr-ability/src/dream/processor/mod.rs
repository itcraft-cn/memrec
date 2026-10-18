//! # DreamProcessor 记忆整合处理器
//!
//! 执行记忆的两段式整合（Stage F 融合 + Stage N 成网），类似人类睡眠中的
//! 记忆巩固过程。
//!
//! ## 流程
//!
//! 1. 筛选符合条件的旧记忆（候选）
//! 2. Stage F 融合：零 LLM 语义聚类 → 分簇 LLM 结构化提炼（事实/摘要/矛盾）
//!    → 写入整合记忆 + provenance → 软删源
//! 3. Stage N 成网：零 LLM 边（Similar/SameProject/References/Contradicts）
//!    → 传播增量
//! 4. 阶段 1-4（交叉项目/个人摘要/清理/向量重生成）
//! 5. 全程写入 DreamRun 审计
//!
//! ## 命名正交
//!
//! 新概念一律用 `DreamRun` / `EdgeBuilder` / `DreamReport`，不复用 `Scene`。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

use mr_common::types::{
    DreamConfig, DreamRun, DreamRunStatus, GateOutcome, Memory, MemoryScope, MemorySource,
    MemoryType, QualityMetrics, SummaryClues,
};

use crate::edge::EdgeGenerator;
use crate::embedding::EmbeddingGenerator;
use crate::llm::{LlmClient, LlmError};
use crate::propagation::PropagationEngine;
use crate::storage::{DreamRunStorage, EdgeStorage, MemoryStorage, VectorStorage};

use super::fusion::{cluster_candidates, extract_cluster};
use super::gate::{DreamGate, DreamGateResult};
use super::lock::DreamLock;
use super::networking::EdgeBuilder;
use super::phases::{
    CrossProjectExtractor, MemoryCleaner, PersonalSummarizer, PhaseResult, VectorRegenerator,
};

/// 整合摘要记忆的标签（幂等与追溯依据）。
const TAG_INTEGRATED: &str = "dream-integrated";
/// 整合事实条的标签。
const TAG_FACT: &str = "dream-fact";
/// 主题摘要的标签。
const TAG_SUMMARY: &str = "dream-summary";

#[derive(Debug, Error)]
pub enum DreamError {
    /// 硬性门控失败：配置/依赖/并发问题导致本次无法执行（如未接入 LLM、锁被占用）。
    #[error("Gate check failed: {0}")]
    GateFailed(String),
    /// 门控正常拦截：并非故障，而是预期性的"暂不执行"。
    ///
    /// 与 [`DreamError::GateFailed`] 区分，调用方（如 `dream_loop`）应对其按
    /// "跳过"处理并降级日志，避免把"距上次执行过近""新增记忆不足"等正常状态
    /// 误报为告警。`outcome` 保留门控原因供审计与上游判定。
    #[error("Dream skipped: {message}")]
    GateSkipped {
        outcome: GateOutcome,
        message: String,
    },
    #[error("Lock error: {0}")]
    Lock(#[from] super::lock::DreamLockError),
    #[error("Storage error: {0:#}")]
    Storage(#[from] anyhow::Error),
    #[error("Not enough memories: {0} < {1}")]
    NotEnoughMemories(usize, usize),
    #[error("LLM error: {0:#}")]
    Llm(#[from] LlmError),
}

/// Dream 执行结果。
#[derive(Debug, Clone)]
pub struct DreamResult {
    /// 本次运行的审计 ID（未接入 run_store 时为 None）
    pub run_id: Option<Uuid>,
    /// 被整合（软删）的源记忆数
    pub integrated_count: usize,
    /// 代表性产出记忆 ID（主题摘要优先）
    pub created_memory_id: Option<Uuid>,
    /// 本次全部摘要的合并文本
    pub summary: String,
    /// 摘要中的关键数据线索
    pub summary_clues: SummaryClues,
    /// 阶段执行结果
    pub phase_results: Vec<PhaseResult>,
    /// 聚类簇数
    pub cluster_count: usize,
    /// 矛盾对数
    pub contradiction_count: usize,
    /// 新建边数
    pub edge_count: usize,
    /// 传播增量数
    pub propagation_count: usize,
    /// 是否预演
    pub dry_run: bool,
    /// 质量度量
    pub metrics: QualityMetrics,
}

impl DreamResult {
    /// 无事可做时的空结果。
    pub(super) fn empty(summary: String, dry_run: bool) -> Self {
        Self {
            run_id: None,
            integrated_count: 0,
            created_memory_id: None,
            summary,
            summary_clues: SummaryClues::default(),
            phase_results: Vec::new(),
            cluster_count: 0,
            contradiction_count: 0,
            edge_count: 0,
            propagation_count: 0,
            dry_run,
            metrics: QualityMetrics::default(),
        }
    }
}

pub struct DreamProcessor {
    config: DreamConfig,
    lock: DreamLock,
    storage: Arc<dyn MemoryStorage>,
    vector_store: Option<Arc<dyn VectorStorage>>,
    embedder: Option<Arc<dyn EmbeddingGenerator>>,
    llm: Option<Arc<dyn LlmClient>>,
    edge_store: Option<Arc<dyn EdgeStorage>>,
    propagation: Option<Arc<PropagationEngine>>,
    run_store: Option<Arc<dyn DreamRunStorage>>,
}

impl DreamProcessor {
    pub fn new(
        config: DreamConfig,
        data_dir: &std::path::Path,
        storage: Arc<dyn MemoryStorage>,
    ) -> Self {
        Self {
            config,
            lock: DreamLock::new(data_dir),
            storage,
            vector_store: None,
            embedder: None,
            llm: None,
            edge_store: None,
            propagation: None,
            run_store: None,
        }
    }

    pub fn with_vector_store(mut self, vector_store: Arc<dyn VectorStorage>) -> Self {
        self.vector_store = Some(vector_store);
        self
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn EmbeddingGenerator>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// 注入 LLM 客户端，用于 Dream 分簇提炼。
    ///
    /// 未注入且 `config.requires_llm` 时，Dream 执行将被 Gate 拦截。
    pub fn with_llm(mut self, llm: Arc<dyn LlmClient>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// 注入边存储，启用 Stage N 成网。
    pub fn with_edge_store(mut self, edge_store: Arc<dyn EdgeStorage>) -> Self {
        self.edge_store = Some(edge_store);
        self
    }

    /// 注入传播引擎，启用整合后的增量传播。
    pub fn with_propagation(mut self, propagation: Arc<PropagationEngine>) -> Self {
        self.propagation = Some(propagation);
        self
    }

    /// 注入 DreamRun 审计存储。
    pub fn with_run_store(mut self, run_store: Arc<dyn DreamRunStorage>) -> Self {
        self.run_store = Some(run_store);
        self
    }

    /// 执行 Dream（不预演）。
    pub async fn execute(&self, force: bool) -> Result<DreamResult, DreamError> {
        self.execute_ext(force, false).await
    }

    /// 执行 Dream，可选预演（dry-run 不写库、不删数据、不建边）。
    ///
    /// # 审计
    ///
    /// 无论成功、无事可做（NoOp）还是失败，只要注入了 `run_store` 均写入
    /// 一条 DreamRun；门控拦截（force=false 时）同样落库。
    pub async fn execute_ext(&self, force: bool, dry_run: bool) -> Result<DreamResult, DreamError> {
        // 当前记忆总数：锁记录与状态写入均基于该值
        let memory_count = self.storage.count().await.map_err(DreamError::from)?;

        if !force {
            // 时间门槛基于持久化的 dream.state（上次执行完成时间），
            // 与并发锁分离：release 锁后时间信息不丢失，24h 门槛不会失效。
            let last_state = self.lock.read_state();

            // P7: 新记忆门槛改为统计"距上次执行后的用户新增记忆数"
            //（created_at > last_run_at 且非 System 来源）。
            let new_user_memories = match &last_state {
                Some(state) => count_new_user_memories(&self.storage, state.last_run_at).await?,
                // 首次执行（无 state）：不设新记忆门槛
                None => memory_count,
            };

            let gate_result =
                DreamGate::check(&self.config, last_state.as_ref(), new_user_memories);

            if gate_result != DreamGateResult::Allowed {
                let (outcome, msg) = gate_outcome(&gate_result);
                let mut run = DreamRun::running(force, dry_run);
                run.gate_outcome = outcome;
                run.status = DreamRunStatus::NoOp;
                run.finish(DreamRunStatus::NoOp);
                run.error = Some(msg.clone());
                self.record_run(&run).await;
                return Err(DreamError::GateSkipped {
                    outcome,
                    message: msg,
                });
            }
        }

        // LLM 依赖检查：requires_llm 且未注入 LLM 客户端时视为未启用
        if self.config.requires_llm && self.llm.is_none() {
            return Err(DreamError::GateFailed(
                "Dream requires LLM, but no LLM client injected".to_string(),
            ));
        }

        if !self.lock.try_acquire(memory_count as u32)? {
            return Err(DreamError::GateFailed(
                "Another Dream process is running".to_string(),
            ));
        }

        let mut run = DreamRun::running(force, dry_run);
        let result = self.process_inner(&mut run, dry_run).await;

        // P2: 仅成功执行才写 state，失败（如 LLM 故障）不刷新 last_run_at，
        // 保证故障恢复后可立即重试，不浪费 24h 重试窗口。
        if result.is_ok() && !dry_run {
            let post_count = self.storage.count().await.map_err(DreamError::from)?;
            if let Err(e) = self.lock.write_state(post_count as u32) {
                tracing::warn!("Failed to write dream state: {}", e);
            }
        }

        // 终态：process_inner 已置 NoOp 则保留，否则 Success / Failed
        match &result {
            Ok(_) => {
                if run.status == DreamRunStatus::Running {
                    run.status = DreamRunStatus::Success;
                }
            }
            Err(e) => {
                run.status = DreamRunStatus::Failed;
                run.error = Some(e.to_string());
            }
        }
        run.finish(run.status);
        self.record_run(&run).await;

        self.lock.release()?;

        result.map(|mut r| {
            r.run_id = Some(run.run_id);
            r
        })
    }

    /// 写入 DreamRun 审计（未注入 run_store 时静默跳过）。
    async fn record_run(&self, run: &DreamRun) {
        if let Some(store) = &self.run_store {
            if let Err(e) = store.save_run(run).await {
                warn!(target: "dream", "Failed to persist DreamRun {}: {}", run.run_id, e);
            }
        }
    }
}

// 子模块：辅助函数 / 主流程 / 度量与阶段（原单文件约 1600 行）。
use helpers::{count_new_user_memories, gate_outcome};
pub use helpers::extract_summary_clues;

mod helpers;
mod metrics;
mod pipeline;

#[cfg(test)]
mod tests;
