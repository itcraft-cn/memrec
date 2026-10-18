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

/// 计算两个等长向量的余弦相似度；维度不一致或存在零向量时返回 0。
///
/// 用于 Dream 上游去重与语义边候选筛选的近似度量。调用方需自行保证向量
/// 已归一化或对结果量级不敏感；本函数不修改入参。
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// 清洗 LLM 摘要输出，剥离常见套话与格式噪音。
///
/// 处理项：Markdown 代码块包裹、常见开场白前缀（循环剥离兼容嵌套）、
/// 连续空行压缩、首尾空白裁剪。不处理结尾客套（误伤风险高，交给 prompt 约束）。
fn clean_summary(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    // 剥离 Markdown 代码块包裹（```markdown ... ``` / ``` ... ```）
    if s.starts_with("```") {
        if let Some(end) = s.rfind("```") {
            if end > 3 {
                // 去掉代码块起始标记后的语言标记行（markdown/json 等）
                let inner = &s[3..end];
                let body = match inner.find('\n') {
                    Some(nl) => &inner[nl + 1..],
                    None => inner,
                };
                s = body.trim().to_string();
            }
        }
    }

    // 循环剥离常见套话前缀（兼容 \"好的，以下是……\" 等多层嵌套）
    const PREFIXES: &[&str] = &[
        "以下是",
        "以下为",
        "以下是我",
        "以下是对",
        "好的，",
        "好的:",
        "好的：",
        "好的。",
        "总结如下",
        "摘要如下",
        "整合结果",
        "整合摘要",
        "整合后的摘要",
        "整合内容",
        "基于以上",
        "根据以上",
        "经过整合",
        "整合完成",
        "已整合",
        "这里",
        "结果如下",
        "内容如下",
    ];
    loop {
        let before = s.clone();
        for prefix in PREFIXES {
            if let Some(rest) = s.strip_prefix(prefix) {
                let rest = rest.trim_start_matches(&[':', '：', '-', '—', '\n'][..]);
                s = rest.trim_start().to_string();
                break;
            }
        }
        if s == before {
            break;
        }
    }

    // 压缩连续空行为单个空行
    let mut compact = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        compact.push_str(line);
        compact.push('\n');
        prev_blank = blank;
    }
    compact.trim_end().to_string()
}

/// 从 Dream 摘要中提取关键数据线索，用于质量评估与追溯。
///
/// 提取项：
/// - 数字数据：如 "75万条"、"4.2KB"、"134MB"、"108MB" 等
/// - 文件路径：如 "xnet_handler.rs:136"、"scenes.toml" 等
/// - 来源线索：如 "日志计数显示"、"通过gdb分析"、"代码review发现" 等
/// - 内存相关：如 "Eden"、"RSS"、"堆外"、"mmap" 等术语
/// - 行号：如 ":136"、":42" 等
pub fn extract_summary_clues(summary: &str) -> SummaryClues {
    let mut clues = SummaryClues::default();

    for line in summary.lines() {
        // 提取数字数据（含单位）
        if let Some(m) =
            lazy_regex::regex!(r"\d+([.,]\d+)?\s*(MB|GB|KB|条|次|行|天|h|小时|分钟|秒)").find(line)
        {
            clues.numbers.push(m.as_str().to_string());
        }

        // 提取文件路径（带或不带行号），完整收集（修复原有空循环死代码）
        for m in lazy_regex::regex!(r"[\w/_.-]+\.(rs|cpp|h|toml|json|log)").find_iter(line) {
            let path = m.as_str().to_string();
            if !clues.file_paths.contains(&path) {
                clues.file_paths.push(path);
            }
        }

        // 提取来源线索
        let source_clues = [
            ("日志", "log-based"),
            ("gdb", "gdb-analysis"),
            ("分析", "analysis"),
            ("计数", "count-based"),
            ("review", "code-review"),
            ("代码", "code-based"),
            ("测试", "test-based"),
            ("验证", "verified"),
        ];
        for (keyword, clue_type) in source_clues {
            if line.to_lowercase().contains(keyword) {
                clues.source_clues.insert(clue_type.to_string());
            }
        }

        // 提取内存相关术语
        let memory_terms = [
            "Eden", "RSS", "堆", "堆外", "mmap", "arena", "jemalloc", "uncommit",
        ];
        for term in memory_terms {
            if line.contains(term) || line.to_lowercase().contains(term) {
                clues.memory_terms.insert(term.to_string());
            }
        }

        // 提取行号（形如 :136），完整收集（修复原有空循环死代码）
        for cap in lazy_regex::regex!(r":\s*(\d+)").captures_iter(line) {
            if let Some(n) = cap.get(1) {
                if let Ok(num) = n.as_str().parse::<u32>() {
                    if !clues.line_numbers.contains(&num) {
                        clues.line_numbers.push(num);
                    }
                }
            }
        }
    }

    clues
}

/// 统计距 `since_unix`（Unix 秒）之后的用户新增记忆数。
///
/// 作为 Gate 新记忆门槛依据：排除 System 来源（Dream 自身产物：
/// 整合记忆/统计摘要），只统计用户侧新增（User/Inferred/External），
/// 避免 Dream 产物被计入导致门槛恒满足。
async fn count_new_user_memories(
    storage: &Arc<dyn MemoryStorage>,
    since_unix: i64,
) -> Result<usize, DreamError> {
    let since =
        DateTime::<Utc>::from_timestamp(since_unix, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    // P0/T1: 按创建时间区间下推查询，避免全量加载后计数。
    let recent = storage
        .list_created_between(since.timestamp(), Utc::now().timestamp(), usize::MAX)
        .await
        .map_err(DreamError::from)?;
    Ok(recent
        .into_iter()
        .filter(|m| m.source != MemorySource::System)
        .count())
}

/// 把门控结果映射为 DreamRun 的 [`GateOutcome`] 与错误信息。
fn gate_outcome(result: &DreamGateResult) -> (GateOutcome, String) {
    match result {
        DreamGateResult::Allowed => (GateOutcome::Allowed, String::new()),
        DreamGateResult::Disabled => (GateOutcome::Disabled, "Dream is disabled".to_string()),
        DreamGateResult::LlmNotConfigured => (
            GateOutcome::LlmNotConfigured,
            "Dream requires LLM, but [llm] is not configured".to_string(),
        ),
        DreamGateResult::TooSoon {
            hours_since_last,
            min_hours,
        } => (
            GateOutcome::TooSoon,
            format!("Too soon: {:.1}h < {:.1}h", hours_since_last, min_hours),
        ),
        DreamGateResult::NotEnoughNewMemories {
            new_memories,
            min_new_memories,
        } => (
            GateOutcome::NotEnoughNew,
            format!(
                "Not enough new memories: {} < {}",
                new_memories, min_new_memories
            ),
        ),
    }
}

/// 解析配置中的整合记忆类型字符串。
fn parse_memory_type(s: &str) -> MemoryType {
    match s {
        "decision" => MemoryType::Decision,
        "knowledge" => MemoryType::Knowledge,
        "context" => MemoryType::Context,
        "preference" => MemoryType::Preference,
        _ => MemoryType::Knowledge,
    }
}

/// 构建一条 Dream 产出记忆（事实条 / 主题摘要）。
///
/// 产出统一为 System 来源、Global 范围、project_id = None（T4 全局语义统一），
/// 以便检索来源权重与可见范围稳定。
fn build_produced_memory(
    content: String,
    memory_type: MemoryType,
    tags: Vec<String>,
    metadata: HashMap<String, String>,
    importance: f32,
) -> Memory {
    Memory {
        id: Uuid::new_v4(),
        project_id: None,
        content,
        memory_type,
        tags,
        importance,
        created_at: Utc::now(),
        last_accessed: Utc::now(),
        access_count: 1,
        source: MemorySource::System,
        scope: MemoryScope::Global,
        summary: None,
        embedding: None,
        metadata,
        is_deleted: false,
        deleted_at: None,
        chunk_group_id: None,
        chunk_index: None,
        chunk_total: None,
    }
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
    fn empty(summary: String, dry_run: bool) -> Self {
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

    async fn process_inner(
        &self,
        run: &mut DreamRun,
        dry_run: bool,
    ) -> Result<DreamResult, DreamError> {
        // 先筛选符合条件的旧记忆；数量不足时直接返回，不执行任何阶段。
        let cutoff = Utc::now() - chrono::Duration::hours(self.config.max_age_hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        // P0/T1: 候选必然早于 cutoff，用时间区间下推替代全量加载。
        let all = self
            .storage
            .list_created_between(0, cutoff.timestamp(), usize::MAX)
            .await
            .map_err(DreamError::from)?;

        // P4: 排除高价值记忆（importance ≥ 阈值、命中保留标签、保留类型），
        // 同时过滤过短内容（测试/占位垃圾记录）。
        let min_content_len = self.config.integration_min_content_len;
        let mut candidates: Vec<Memory> = all
            .into_iter()
            .filter(|m| !m.is_deleted && m.created_at < cutoff)
            .filter(|m| {
                m.importance < self.config.integration_preserve_importance
                    && !m
                        .tags
                        .iter()
                        .any(|t| self.config.integration_preserve_tags.contains(t))
                    && !self
                        .config
                        .integration_preserve_types
                        .iter()
                        .any(|t| t == &m.memory_type.to_string())
                    && m.content.trim().chars().count() >= min_content_len
            })
            .collect();

        // P5: 排除已被整合的源记忆；上次崩溃残留的已整合源记忆补删，保证幂等。
        let seen = self.collect_integrated_source_ids().await?;
        if !seen.is_empty() {
            let mut leftover: Vec<Uuid> = Vec::new();
            candidates.retain(|m| {
                if seen.contains(&m.id) {
                    leftover.push(m.id);
                    false
                } else {
                    true
                }
            });
            if !dry_run {
                for id in leftover {
                    if let Err(e) = self.storage.delete(&id).await {
                        warn!("Failed to delete leftover integrated memory {}: {}", id, e);
                    }
                }
            }
        }

        // P1/P3: 按重要性升序（优先整合低价值记忆），截取单次整合上限。
        candidates.sort_by(|a, b| {
            a.importance
                .partial_cmp(&b.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 门禁先于截断：以“可整合候选总数”判定是否达到最小门槛，避免
        // integration_max_memories < min_memories 时被截断后误报“记忆不足”。
        if candidates.len() < self.config.min_memories {
            warn!(
                "Not enough memories for Dream integration: {} < {}",
                candidates.len(),
                self.config.min_memories
            );
            run.status = DreamRunStatus::NoOp;
            return Ok(DreamResult::empty(
                "Not enough old memories for Dream integration".to_string(),
                dry_run,
            ));
        }

        candidates.truncate(self.config.integration_max_memories);
        run.candidate_count = candidates.len();

        info!(
            "Dream processing {} memories older than {}",
            candidates.len(),
            cutoff_str
        );

        // ---- Stage F 融合：零 LLM 聚类 ----
        let candidate_emb = self.collect_embeddings(&candidates).await;
        let clusters: Vec<Vec<usize>> = if self.config.fusion_cluster_enabled {
            cluster_candidates(
                &candidates,
                &candidate_emb,
                self.config.fusion_cluster_threshold,
                self.config.fusion_cluster_max_size,
                self.config.fusion_max_clusters,
            )
        } else {
            vec![(0..candidates.len()).collect()]
        };
        run.cluster_count = clusters.len();

        // 分簇提炼：先完成全部簇的提炼（LLM 故障在写入前即中止，保证一致性）。
        let use_llm = !dry_run || self.config.dry_run_with_llm;
        let mut extractions: Vec<(Vec<usize>, super::fusion::ClusterExtraction)> =
            Vec::with_capacity(clusters.len());
        for cluster in &clusters {
            if cluster.is_empty() {
                continue;
            }
            let cluster_mems: Vec<Memory> =
                cluster.iter().map(|&i| candidates[i].clone()).collect();
            match extract_cluster(self.llm.as_ref(), &cluster_mems, use_llm).await {
                Ok(x) => extractions.push((cluster.clone(), x)),
                Err(e) => return Err(DreamError::Llm(e)),
            }
        }

        // 写入产出（dry-run 时只统计不落库）。
        let mut integrated_ids: Vec<Uuid> = Vec::new();
        let mut summary_ids: Vec<Uuid> = Vec::new();
        let mut produced: Vec<Memory> = Vec::new();
        let mut references: Vec<(Uuid, Uuid)> = Vec::new();
        let mut contradictions: Vec<(Uuid, Uuid, String)> = Vec::new();
        let mut summary_texts: Vec<String> = Vec::new();

        let mem_type = parse_memory_type(&self.config.integration_type);
        // 上游去重：跨簇累积已保留事实条的嵌入，用于近重复判定。
        let mut kept_fact_embs: Vec<Vec<f32>> = Vec::new();
        let mut dedup_dropped = 0usize;

        for (cluster, extraction) in &extractions {
            let source_ids: Vec<String> = cluster
                .iter()
                .map(|&i| candidates[i].id.to_string())
                .collect();
            let mut base_meta: HashMap<String, String> = HashMap::new();
            base_meta.insert(
                "source_ids".to_string(),
                serde_json::to_string(&source_ids).unwrap_or_default(),
            );
            base_meta.insert("dream_run_id".to_string(), run.run_id.to_string());
            base_meta.insert("integrated_count".to_string(), cluster.len().to_string());

            // 事实条
            if self.config.fusion_emit_facts {
                for fact in &extraction.facts {
                    let mut meta = base_meta.clone();
                    if !fact.source_hints.is_empty() {
                        meta.insert(
                            "source_hints".to_string(),
                            serde_json::to_string(&fact.source_hints).unwrap_or_default(),
                        );
                    }
                    let mut tags = self.config.integration_tags.clone();
                    tags.push(TAG_INTEGRATED.to_string());
                    tags.push(TAG_FACT.to_string());
                    let mut m = build_produced_memory(fact.text.clone(), mem_type, tags, meta, 0.7);

                    // 上游去重：事实条与已保留事实条近重复（余弦 >= 阈值）则丢弃，
                    // 仅保留最先产出的一条，避免梦产出冗余记忆与"重复当关系"的噪声边。
                    if self.config.fusion_dedup_enabled {
                        if let Some(emb) = self
                            .embedder
                            .as_ref()
                            .and_then(|e| e.embed(&m.content).ok())
                        {
                            if kept_fact_embs.iter().any(|k| {
                                cosine_similarity(k, &emb) >= self.config.fusion_dedup_threshold
                            }) {
                                dedup_dropped += 1;
                                continue;
                            }
                            m.embedding = Some(emb.clone());
                            kept_fact_embs.push(emb);
                        }
                    }

                    let mid = m.id;
                    if !dry_run {
                        self.storage.add(&m).await.map_err(DreamError::from)?;
                    }
                    integrated_ids.push(mid);
                    produced.push(m);
                    // 事实条引用本簇全部源
                    for &i in cluster {
                        references.push((mid, candidates[i].id));
                    }
                }
            }

            // 主题摘要
            if self.config.fusion_emit_summary {
                let content = clean_summary(&extraction.summary);
                let mut tags = self.config.integration_tags.clone();
                tags.push(TAG_INTEGRATED.to_string());
                tags.push(TAG_SUMMARY.to_string());
                let m =
                    build_produced_memory(content.clone(), mem_type, tags, base_meta.clone(), 0.8);
                let mid = m.id;
                if !dry_run {
                    self.storage.add(&m).await.map_err(DreamError::from)?;
                }
                summary_ids.push(mid);
                integrated_ids.push(mid);
                summary_texts.push(content);
                produced.push(m);
            }

            // 矛盾对
            if self.config.fusion_detect_contradictions {
                for c in &extraction.contradictions {
                    if c.a < cluster.len() && c.b < cluster.len() && c.a != c.b {
                        contradictions.push((
                            candidates[cluster[c.a]].id,
                            candidates[cluster[c.b]].id,
                            c.reason.clone(),
                        ));
                    }
                }
            }
        }

        if dedup_dropped > 0 {
            tracing::info!(
                dropped = dedup_dropped,
                threshold = self.config.fusion_dedup_threshold,
                "Dream Stage F 上游去重：丢弃近重复事实条"
            );
        }

        run.integrated_memory_ids = integrated_ids.clone();
        run.summary_memory_id = summary_ids.last().copied();
        run.contradiction_count = contradictions.len();

        let summary_joined = summary_texts.join("\n\n");
        let summary_clues = extract_summary_clues(&summary_joined);
        let created_memory_id = summary_ids
            .last()
            .copied()
            .or_else(|| integrated_ids.first().copied());

        // 预演：到此为止，不删源、不建边、不执行阶段。
        if dry_run {
            let metrics = self.compute_metrics(
                &candidates,
                &produced,
                run.cluster_count,
                &contradictions,
                0,
                0,
                0.0,
            );
            run.metrics = metrics.clone();
            run.finish(DreamRunStatus::Success);
            return Ok(DreamResult {
                run_id: Some(run.run_id),
                integrated_count: candidates.len(),
                created_memory_id,
                summary: summary_joined,
                summary_clues,
                phase_results: Vec::new(),
                cluster_count: run.cluster_count,
                contradiction_count: contradictions.len(),
                edge_count: 0,
                propagation_count: 0,
                dry_run: true,
                metrics,
            });
        }

        // 软删源记忆
        for memory in &candidates {
            self.storage
                .delete(&memory.id)
                .await
                .map_err(DreamError::from)?;
            run.deleted_source_ids.push(memory.id);
        }

        // ---- Stage N 成网：零 LLM 边 ----
        let mut edge_count = 0usize;
        if self.config.networking_enabled {
            if let Some(edge_store) = &self.edge_store {
                let produced_emb = self.collect_embeddings(&produced).await;
                let mut pool_emb = candidate_emb.clone();
                for (k, v) in &produced_emb {
                    pool_emb.insert(*k, v.clone());
                }
                let mut pool = candidates.clone();
                pool.extend(produced.iter().cloned());
                let builder = EdgeBuilder::new(self.config.clone());
                match builder
                    .build(
                        &produced,
                        &produced_emb,
                        &pool,
                        &pool_emb,
                        &references,
                        &contradictions,
                        edge_store,
                    )
                    .await
                {
                    Ok(stats) => {
                        edge_count = stats.total;
                        run.edge_count = stats.total;
                        run.edge_counts_by_type = stats.by_type;
                    }
                    Err(e) => warn!(target: "dream", "Edge build failed: {}", e),
                }

                // Stage N 语义边（P2-8，默认关）：LLM 判断 CausedBy/Evolves/References
                if self.config.networking_semantic_enabled {
                    if let Some(llm) = &self.llm {
                        let gen = crate::edge::LlmEdgeGenerator::new(llm.clone(), 3, 0.6);
                        let max_candidates = self.config.networking_semantic_max_candidates;
                        for m in &produced {
                            // 语义边候选池限幅：按向量相似度取最相近的 top-N，排除自身与
                            // 近重复（近重复应走上游去重，而非建立关系边），避免把全量
                            // 候选+产出整体入参导致成本随规模线性膨胀。
                            let m_emb = produced_emb.get(&m.id);
                            let mut scored: Vec<(&Memory, f32)> = Vec::new();
                            for cand in &pool {
                                if cand.id == m.id {
                                    continue;
                                }
                                let s = match (m_emb, pool_emb.get(&cand.id)) {
                                    (Some(a), Some(b)) => cosine_similarity(a, b),
                                    _ => 0.0,
                                };
                                if s >= self.config.networking_semantic_dup_threshold {
                                    continue;
                                }
                                scored.push((cand, s));
                            }
                            scored.sort_by(|a, b| {
                                b.1.partial_cmp(&a.1)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                                    .then_with(|| a.0.id.cmp(&b.0.id))
                            });
                            let candidates_limited: Vec<Memory> = if max_candidates == 0 {
                                Vec::new()
                            } else {
                                scored
                                    .iter()
                                    .take(max_candidates)
                                    .map(|(c, _)| (*c).clone())
                                    .collect()
                            };

                            match gen
                                .generate(m, &candidates_limited, edge_store.as_ref())
                                .await
                            {
                                Ok(edges) => {
                                    for e in edges {
                                        if let Err(err) = edge_store.save(&e).await {
                                            warn!(target: "dream", "Save semantic edge failed: {}", err);
                                        } else {
                                            edge_count += 1;
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(target: "dream", "Semantic edge generation failed: {}", e)
                                }
                            }
                        }
                        run.edge_count = edge_count;
                    }
                }
            }
        }

        // 传播增量
        let mut propagation_count = 0usize;
        if self.config.networking_propagation_enabled {
            if let Some(engine) = &self.propagation {
                for m in &produced {
                    match engine.propagate(m).await {
                        Ok(r) => propagation_count += r.delta_count,
                        Err(e) => warn!(target: "dream", "Propagation failed for {}: {}", m.id, e),
                    }
                }
            }
        }
        run.propagation_count = propagation_count;

        // 阶段 1-4
        let phase_results = self.run_phases().await;
        run.phase_results = phase_results.iter().map(|p| p.to_record()).collect();

        let phase_success_rate = if phase_results.is_empty() {
            1.0
        } else {
            phase_results.iter().filter(|p| p.success).count() as f32 / phase_results.len() as f32
        };
        let metrics = self.compute_metrics(
            &candidates,
            &produced,
            run.cluster_count,
            &contradictions,
            edge_count,
            propagation_count,
            phase_success_rate,
        );
        run.metrics = metrics.clone();

        run.finish(DreamRunStatus::Success);

        info!(
            "Dream completed: {} memories integrated, {} edges, {} propagations",
            candidates.len(),
            edge_count,
            propagation_count
        );

        Ok(DreamResult {
            run_id: Some(run.run_id),
            integrated_count: candidates.len(),
            created_memory_id,
            summary: summary_joined,
            summary_clues,
            phase_results,
            cluster_count: run.cluster_count,
            contradiction_count: contradictions.len(),
            edge_count,
            propagation_count,
            dry_run: false,
            metrics,
        })
    }

    /// 执行阶段 1-4（交叉项目 / 个人摘要 / 清理 / 向量重生成）。
    async fn run_phases(&self) -> Vec<PhaseResult> {
        let mut phase_results = Vec::new();

        if self.config.phase_cross_project {
            let extractor = CrossProjectExtractor::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            phase_results.push(extractor.execute().await);
        }

        if self.config.phase_personal_summary {
            let summarizer = PersonalSummarizer::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            phase_results.push(summarizer.execute().await);
        }

        if self.config.phase_cleanup {
            let cleaner = MemoryCleaner::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
                self.config.cleanup_inactive_days,
                self.config.cleanup_importance_threshold,
            );
            phase_results.push(cleaner.execute().await);
        }

        if self.config.phase_vector_regen {
            if let (Some(vector_store), Some(embedder)) = (&self.vector_store, &self.embedder) {
                let regenerator = VectorRegenerator::new(
                    self.storage.clone(),
                    vector_store.clone(),
                    embedder.clone(),
                    self.config.batch_size,
                    self.config.batch_interval_ms,
                );
                phase_results.push(regenerator.execute().await);
            }
        }

        phase_results
    }

    /// 收集记忆向量（优先级：memory.embedding → vector_store → embedder）。
    async fn collect_embeddings(&self, mems: &[Memory]) -> HashMap<Uuid, Vec<f32>> {
        let mut map = HashMap::new();
        for m in mems {
            if let Some(v) = &m.embedding {
                if !v.is_empty() {
                    map.insert(m.id, v.clone());
                    continue;
                }
            }
            if let Some(vs) = &self.vector_store {
                if let Ok(Some(v)) = vs.get(&m.id).await {
                    if !v.is_empty() {
                        map.insert(m.id, v);
                        continue;
                    }
                }
            }
            if let Some(e) = &self.embedder {
                if let Ok(v) = e.embed(&m.content) {
                    if !v.is_empty() {
                        map.insert(m.id, v);
                    }
                }
            }
        }
        map
    }

    /// 计算 DreamRun 质量度量。
    #[allow(clippy::too_many_arguments)]
    fn compute_metrics(
        &self,
        sources: &[Memory],
        produced: &[Memory],
        cluster_count: usize,
        contradictions: &[(Uuid, Uuid, String)],
        edge_count: usize,
        propagation_count: usize,
        phase_success_rate: f32,
    ) -> QualityMetrics {
        let input_chars: usize = sources.iter().map(|m| m.content.chars().count()).sum();
        let output_chars: usize = produced.iter().map(|m| m.content.chars().count()).sum();
        let input_text: String = sources
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let output_text: String = produced
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let in_clues = extract_summary_clues(&input_text).total();
        let out_clues = extract_summary_clues(&output_text).total();
        let clue_retention = if in_clues == 0 {
            1.0
        } else {
            (out_clues.min(in_clues) as f32) / (in_clues as f32)
        };
        let compression_ratio = if output_chars == 0 {
            0.0
        } else {
            input_chars as f32 / output_chars as f32
        };
        let avg_cluster_size = if cluster_count == 0 {
            0.0
        } else {
            sources.len() as f32 / cluster_count as f32
        };
        QualityMetrics {
            clue_retention,
            compression_ratio,
            avg_cluster_size,
            contradiction_count: contradictions.len(),
            edge_count,
            propagation_count,
            phase_success_rate,
        }
    }

    /// 收集所有已写入整合记忆（`dream-integrated` 标签）记录的源记忆 id。
    ///
    /// 幂等机制：整合记忆写入时把源记忆 id 列表存入 `metadata.source_ids`，
    /// 下次执行据此排除已整合的源记忆，崩溃残留（写后未删）也可识别补删。
    async fn collect_integrated_source_ids(&self) -> Result<HashSet<Uuid>, DreamError> {
        let mut seen = HashSet::new();
        // P0/T2: 空 limit 表示不截断。原硬编码 1000 会在整合记忆超限时漏判源记忆，
        // 破坏幂等补删语义。
        let summaries = self
            .storage
            .list_by_tag(TAG_INTEGRATED, usize::MAX)
            .await
            .map_err(DreamError::from)?;
        for m in summaries {
            if let Some(raw) = m.metadata.get("source_ids") {
                if let Ok(ids) = serde_json::from_str::<Vec<String>>(raw) {
                    for s in ids {
                        if let Ok(id) = Uuid::parse_str(&s) {
                            seen.insert(id);
                        }
                    }
                }
            }
        }
        Ok(seen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    /// T5 验证：DreamError 经 #[from] 保留原始错误链，Display 输出包裹层
    /// 与根因两层信息（旧 String 化实现会丢根因）。
    #[test]
    fn test_dream_error_preserves_chain() {
        let inner = anyhow::anyhow!("inner root cause").context("storage layer ctx");
        let display = DreamError::from(inner).to_string();
        assert!(display.contains("Storage error: storage layer ctx"));
        assert!(display.contains("inner root cause"));

        let llm_err = DreamError::Llm(LlmError::EmptyResponse).to_string();
        assert!(llm_err.contains("LLM error: LLM response is empty"));
    }

    fn create_test_processor() -> (DreamProcessor, Arc<MemoryStore>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            min_hours_between: 0.0,
            min_memories: 2,
            max_age_hours: 1,
            requires_llm: false,
            ..Default::default()
        };

        let processor = DreamProcessor::new(config, dir.path(), storage.clone());
        (processor, storage, dir)
    }

    #[tokio::test]
    async fn test_execute_disabled() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: false,
            ..Default::default()
        };

        let processor = DreamProcessor::new(config, dir.path(), storage);

        let result = processor.execute(false).await;
        assert!(matches!(result, Err(DreamError::GateSkipped { .. })));
    }

    #[tokio::test]
    async fn test_execute_not_enough_memories() {
        let (processor, storage, _dir) = create_test_processor();

        let mut memory = Memory::new("single memory".to_string(), MemoryType::Knowledge);
        memory.created_at = Utc::now() - chrono::Duration::hours(2);
        storage.save(&memory).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 0);
        assert!(dream_result.created_memory_id.is_none());
    }

    #[tokio::test]
    async fn test_execute_success() {
        let (processor, storage, _dir) = create_test_processor();

        for i in 0..3 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            memory.created_at = Utc::now() - chrono::Duration::hours(2);
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_ok());

        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 3);
        assert!(dream_result.created_memory_id.is_some());
        assert!(!dream_result.summary.is_empty());
        assert!(!dream_result.phase_results.is_empty());
    }

    #[test]
    fn test_clean_summary_strips_code_fence() {
        let raw = "```markdown\n## 主题\nRust 异步性能优化\n```";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "## 主题\nRust 异步性能优化");
    }

    #[test]
    fn test_clean_summary_strips_small_talk_prefix() {
        let raw = "好的，以下是整合后的摘要：\n\n## 主题\n认证方案选型";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "## 主题\n认证方案选型");
    }

    #[test]
    fn test_clean_summary_strips_nested_prefixes() {
        let raw = "好的，以下为总结如下：项目采用 JWT 认证";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "项目采用 JWT 认证");
    }

    #[test]
    fn test_clean_summary_keeps_content() {
        let raw = "## 要点\n- JWT 无状态易扩展";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, raw);
    }

    #[test]
    fn test_clean_summary_compacts_blank_lines() {
        let raw = "## 主题\n\n\n\n## 要点\n- a\n\n\n- b";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "## 主题\n\n## 要点\n- a\n\n- b");
    }

    #[test]
    fn test_dream_result_debug() {
        let result = DreamResult {
            run_id: None,
            integrated_count: 5,
            created_memory_id: Some(Uuid::nil()),
            summary: "test summary".to_string(),
            summary_clues: SummaryClues::default(),
            phase_results: vec![],
            cluster_count: 1,
            contradiction_count: 0,
            edge_count: 0,
            propagation_count: 0,
            dry_run: false,
            metrics: QualityMetrics::default(),
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("integrated_count"));
        assert!(debug_str.contains("5"));
    }

    #[tokio::test]
    async fn test_execute_requires_llm_without_llm_fails() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: true,
            ..Default::default()
        };

        let processor = DreamProcessor::new(config, dir.path(), storage);

        let result = processor.execute(true).await;
        assert!(matches!(result, Err(DreamError::GateFailed(_))));
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("LLM"));
    }

    #[tokio::test]
    async fn test_execute_with_llm_uses_llm_summary() {
        use crate::llm::MockLlmClient;
        use std::sync::Arc as StdArc;

        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let llm = MockLlmClient::new(
            r#"{"facts":[{"text":"fact from llm"}],"summary":"LLM 整合摘要内容","contradictions":[]}"#,
        );

        let config = DreamConfig {
            enabled: true,
            requires_llm: true,
            min_hours_between: 0.0,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };

        let processor =
            DreamProcessor::new(config, dir.path(), storage.clone()).with_llm(StdArc::new(llm));

        for i in 0..3 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            // 记忆年龄须大于 max_age_hours（100h），才会被选中
            memory.created_at = Utc::now() - chrono::Duration::hours(200);
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 3);
        assert_eq!(dream_result.summary, "LLM 整合摘要内容");
        assert!(dream_result.created_memory_id.is_some());
    }

    #[tokio::test]
    async fn test_execute_failure_does_not_write_state() {
        // P2/P6: LLM 失败时 execute 返回 Err；不写 state（不消耗 24h 重试窗口），
        // 且 phases 不执行（无 cross-project 摘要写入）。
        use crate::llm::MockLlmClient;
        use std::sync::Arc as StdArc;

        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        // unconfigured Mock：chat 返回 Err
        let llm = MockLlmClient::unconfigured();

        let config = DreamConfig {
            enabled: true,
            requires_llm: true,
            min_memories: 1,
            max_age_hours: 100,
            ..Default::default()
        };

        let processor =
            DreamProcessor::new(config, dir.path(), storage.clone()).with_llm(StdArc::new(llm));

        for i in 0..3 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            memory.created_at = Utc::now() - chrono::Duration::hours(200);
            memory.importance = 0.5;
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_err());

        // P2: 失败不写 state → 下次仍可立即重试
        let lock = crate::dream::DreamLock::new(dir.path());
        assert!(lock.read_state().is_none());

        // P6: phases 未执行，无 cross-project 摘要
        let summaries = storage.list_by_tag("cross-project", 10).await.unwrap();
        assert!(summaries.is_empty());
    }

    #[tokio::test]
    async fn test_integration_preserves_high_value_memories() {
        // P4: critical 标签 / 高 importance / preference 类型不参与整合与删除
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        // 普通旧记忆：应被整合
        let mut normal = Memory::new("normal old memory".to_string(), MemoryType::Knowledge);
        normal.created_at = Utc::now() - chrono::Duration::hours(200);
        normal.importance = 0.5;
        storage.save(&normal).await.unwrap();

        // critical 标签记忆：应保留
        let mut critical = Memory::new("critical decision".to_string(), MemoryType::Decision);
        critical.created_at = Utc::now() - chrono::Duration::hours(200);
        critical.importance = 0.5;
        critical.tags = vec!["critical".to_string()];
        storage.save(&critical).await.unwrap();

        // 高 importance 记忆：应保留
        let mut high = Memory::new("high importance".to_string(), MemoryType::Knowledge);
        high.created_at = Utc::now() - chrono::Duration::hours(200);
        high.importance = 0.9;
        storage.save(&high).await.unwrap();

        // preference 类型：应保留
        let mut pref = Memory::new("user preference".to_string(), MemoryType::Preference);
        pref.created_at = Utc::now() - chrono::Duration::hours(200);
        pref.importance = 0.5;
        storage.save(&pref).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 1);

        // 被保留的记忆仍在库中（未软删）
        let all = storage.list(100).await.unwrap();
        let alive: Vec<_> = all.into_iter().filter(|m| !m.is_deleted).collect();
        assert!(alive.iter().any(|m| m.id == critical.id));
        assert!(alive.iter().any(|m| m.id == high.id));
        assert!(alive.iter().any(|m| m.id == pref.id));
        assert!(!alive.iter().any(|m| m.id == normal.id));
    }

    #[tokio::test]
    async fn test_integration_skips_already_integrated_sources() {
        // P5: 已整合源记忆（metadata.source_ids 命中）被排除，且残留被补删
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        // 模拟上次崩溃残留：源记忆未被删除，但已有整合记忆记录了它
        let mut leftover = Memory::new("leftover source".to_string(), MemoryType::Knowledge);
        leftover.created_at = Utc::now() - chrono::Duration::hours(200);
        leftover.importance = 0.5;
        storage.save(&leftover).await.unwrap();

        let mut integrated = Memory::new("previous summary".to_string(), MemoryType::Knowledge);
        integrated.tags = vec!["dream-integrated".to_string()];
        integrated.created_at = Utc::now() - chrono::Duration::hours(50);
        let mut md = std::collections::HashMap::new();
        md.insert(
            "source_ids".to_string(),
            serde_json::to_string(&vec![leftover.id.to_string()]).unwrap(),
        );
        integrated.metadata = md;
        storage.save(&integrated).await.unwrap();

        // 再加一条真正的新旧记忆，保证候选数达到 min_memories
        let mut fresh = Memory::new("fresh candidate".to_string(), MemoryType::Knowledge);
        fresh.created_at = Utc::now() - chrono::Duration::hours(200);
        fresh.importance = 0.5;
        storage.save(&fresh).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        // 仅整合 fresh（leftover 已被排除并补删）
        assert_eq!(dream_result.integrated_count, 1);

        let all = storage.list(100).await.unwrap();
        let alive: Vec<_> = all.into_iter().filter(|m| !m.is_deleted).collect();
        assert!(!alive.iter().any(|m| m.id == leftover.id));
        assert!(!alive.iter().any(|m| m.id == fresh.id));
    }

    #[tokio::test]
    async fn test_integration_preserves_context_and_short_content() {
        // P4 扩展: context 类型（配置/环境信息）默认保留；过短内容（测试/占位）不整合
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        // 普通旧知识：应整合
        let mut knowledge = Memory::new(
            "regular old knowledge about glibc arena".to_string(),
            MemoryType::Knowledge,
        );
        knowledge.created_at = Utc::now() - chrono::Duration::hours(200);
        knowledge.importance = 0.5;
        storage.save(&knowledge).await.unwrap();

        // context 类型：应保留（配置细节不可压缩）
        let mut ctx = Memory::new(
            "cotael-srv 配置路径: scenes.toml 账目体系, scheduler.toml 任务调度".to_string(),
            MemoryType::Context,
        );
        ctx.created_at = Utc::now() - chrono::Duration::hours(200);
        ctx.importance = 0.5;
        storage.save(&ctx).await.unwrap();

        // 过短内容（测试垃圾）：不参与整合，保留在库中
        let mut junk = Memory::new("测试".to_string(), MemoryType::Knowledge);
        junk.created_at = Utc::now() - chrono::Duration::hours(200);
        junk.importance = 0.5;
        storage.save(&junk).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        // 仅整合 knowledge（context 与过短内容被排除）
        assert_eq!(dream_result.integrated_count, 1);

        let all = storage.list(100).await.unwrap();
        let alive: Vec<_> = all.into_iter().filter(|m| !m.is_deleted).collect();
        assert!(alive.iter().any(|m| m.id == ctx.id), "context 应保留");
        assert!(alive.iter().any(|m| m.id == junk.id), "过短内容应保留");
        assert!(!alive.iter().any(|m| m.id == knowledge.id));
    }

    #[tokio::test]
    async fn test_integration_respects_max_memories() {
        // P1: integration_max_memories 限制单次整合输入量
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            integration_max_memories: 2,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        for i in 0..5 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            memory.created_at = Utc::now() - chrono::Duration::hours(200);
            memory.importance = 0.5;
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 2);
    }

    #[test]
    fn test_cosine_similarity_basic() {
        // 同向量相似度为 1，正交为 0，维度不一致或零向量为 0
        assert!((super::cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!((super::cosine_similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
        assert_eq!(super::cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(super::cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        // 近重复向量相似度应高于去重阈值 0.97
        let a = [1.0f32, 2.0, 3.0, 4.0];
        let b = [1.0f32, 2.0, 3.0, 4.01];
        assert!(super::cosine_similarity(&a, &b) >= 0.97);
    }
}
