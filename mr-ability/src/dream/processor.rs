//! # DreamProcessor 记忆整合处理器
//!
//! 执行记忆压缩和整合，类似人类睡眠中的记忆巩固过程。
//!
//! ## 流程
//!
//! 1. 筛选符合条件的旧记忆
//! 2. 调用 LLM 整合生成摘要
//! 3. 写入新记忆，删除旧记忆

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

use mr_common::types::{DreamConfig, Memory, MemoryScope, MemorySource, MemoryType};

use crate::embedding::EmbeddingGenerator;
use crate::llm::{LlmClient, LlmMessage};
use crate::storage::{MemoryStorage, VectorStorage};

use super::gate::{DreamGate, DreamGateResult};
use super::lock::DreamLock;
use super::phases::{
    CrossProjectExtractor, MemoryCleaner, PersonalSummarizer, PhaseResult, VectorRegenerator,
};

#[derive(Debug, Error)]
pub enum DreamError {
    #[error("Gate check failed: {0}")]
    GateFailed(String),
    #[error("Lock error: {0}")]
    Lock(#[from] super::lock::DreamLockError),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Not enough memories: {0} < {1}")]
    NotEnoughMemories(usize, usize),
    #[error("LLM error: {0}")]
    Llm(String),
}

/// Dream 整合摘要的 LLM System Prompt（模板约束）。
///
/// 要求 LLM 严格按固定 Markdown 模板输出，并通过硬性约束压制套话：
/// 禁止开场白/结尾寒暄、禁止空泛概括、信息必须来自输入记忆、语言与输入一致、限长。
/// 产出为结构化内容（主题/要点/结论），便于后续检索与阅读。
const SUMMARY_SYSTEM_PROMPT: &str = "你是一个记忆整合引擎，职责是从历史记忆中提炼事实并整合为结构化摘要。\
输出必须严格遵循以下 Markdown 模板，不得增删章节、不得改变章节顺序：\n\
## 主题\n\
（用一句话概括这批记忆的核心主题）\n\
## 要点\n\
（逐条列出最有价值的独立事实，每条一行，以 \"- \" 开头，最多 10 条）\n\
## 结论\n\
（提炼跨记忆的关联、规律或后续行动建议，2-3 行）\n\
硬性约束：\n\
1. 直接输出模板内容，禁止任何开场白（如\"以下是\"\"好的\"\"总结如下\"），禁止结尾寒暄、禁止自我评价；\n\
2. 所有内容必须来自给定记忆，不得编造、不得添加记忆中没有的信息；\n\
3. 不要输出空泛的套话（如\"这些记忆涵盖了多个方面\"），每条要点都必须是具体事实；\n\
4. 语言风格与输入记忆保持一致（中文记忆用中文输出，英文记忆用英文输出）；\n\
5. 总长度不超过 500 字。";

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
    let all = storage
        .list(usize::MAX)
        .await
        .map_err(|e| DreamError::Storage(e.to_string()))?;
    Ok(all
        .into_iter()
        .filter(|m| !m.is_deleted && m.created_at > since && m.source != MemorySource::System)
        .count())
}

#[derive(Debug, Clone)]
pub struct DreamResult {
    pub integrated_count: usize,
    pub created_memory_id: Option<Uuid>,
    pub summary: String,
    pub phase_results: Vec<PhaseResult>,
}

pub struct DreamProcessor {
    config: DreamConfig,
    lock: DreamLock,
    storage: Arc<dyn MemoryStorage>,
    vector_store: Option<Arc<dyn VectorStorage>>,
    embedder: Option<Arc<dyn EmbeddingGenerator>>,
    llm: Option<Arc<dyn LlmClient>>,
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

    /// 注入 LLM 客户端，用于 Dream 整合摘要。
    ///
    /// 未注入且 `config.requires_llm` 时，Dream 执行将被 Gate 拦截。
    pub fn with_llm(mut self, llm: Arc<dyn LlmClient>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub async fn execute(&self, force: bool) -> Result<DreamResult, DreamError> {
        // 当前记忆总数：锁记录与状态写入均基于该值
        let memory_count = self
            .storage
            .count()
            .await
            .map_err(|e| DreamError::Storage(e.to_string()))?;

        if !force {
            // 时间门槛基于持久化的 dream.state（上次执行完成时间），
            // 与并发锁分离：release 锁后时间信息不丢失，24h 门槛不会失效。
            let last_state = self.lock.read_state();

            // P7: 新记忆门槛改为统计"距上次执行后的用户新增记忆数"
            //（created_at > last_run_at 且非 System 来源），
            // 不再用记忆总数相减（净增量会被用户删/加抵消而误拦截，
            // 且 Dream 自身产物会被计入导致门槛恒满足）。
            let new_user_memories = match &last_state {
                Some(state) => count_new_user_memories(&self.storage, state.last_run_at).await?,
                // 首次执行（无 state）：不设新记忆门槛
                None => memory_count,
            };

            let gate_result =
                DreamGate::check(&self.config, last_state.as_ref(), new_user_memories);

            match gate_result {
                DreamGateResult::Allowed => {}
                DreamGateResult::Disabled => {
                    return Err(DreamError::GateFailed("Dream is disabled".to_string()));
                }
                DreamGateResult::LlmNotConfigured => {
                    return Err(DreamError::GateFailed(
                        "Dream requires LLM, but [llm] is not configured".to_string(),
                    ));
                }
                DreamGateResult::TooSoon {
                    hours_since_last,
                    min_hours,
                } => {
                    return Err(DreamError::GateFailed(format!(
                        "Too soon: {:.1}h < {:.1}h",
                        hours_since_last, min_hours
                    )));
                }
                DreamGateResult::NotEnoughNewMemories {
                    new_memories,
                    min_new_memories,
                } => {
                    return Err(DreamError::GateFailed(format!(
                        "Not enough new memories: {} < {}",
                        new_memories, min_new_memories
                    )));
                }
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

        let result = self.process_inner().await;

        // P2: 仅成功执行才写 state，失败（如 LLM 故障）不刷新 last_run_at，
        // 保证故障恢复后可立即重试，不浪费 24h 重试窗口。
        // 整合数不足/无候选属于"成功执行但无事可做"，同样记录（避免空转）。
        if result.is_ok() {
            let post_count = self
                .storage
                .count()
                .await
                .map_err(|e| DreamError::Storage(e.to_string()))?;
            if let Err(e) = self.lock.write_state(post_count as u32) {
                tracing::warn!("Failed to write dream state: {}", e);
            }
        }

        self.lock.release()?;

        result
    }

    async fn process_inner(&self) -> Result<DreamResult, DreamError> {
        // 先筛选符合条件的旧记忆；数量不足时直接返回，
        // 不执行任何阶段（避免无意义的统计摘要写入，防止垃圾记忆堆积）。
        let cutoff = Utc::now() - chrono::Duration::hours(self.config.max_age_hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        // 全量取回后内存筛选：list_older_than 内部 list(10000) 有扫描上限
        // （活跃记忆超 1 万时 7 天前记忆可能漏选），且按 UUID 序无优先级。
        // 这里改为全量扫描 + 按重要性排序选取（P1/P3）。
        let all = self
            .storage
            .list(usize::MAX)
            .await
            .map_err(|e| DreamError::Storage(e.to_string()))?;

        // P4: 排除高价值记忆（importance ≥ 阈值、命中保留标签、Preference 类型），
        // 长期价值记忆不参与整合与删除。
        let mut candidates: Vec<Memory> = all
            .into_iter()
            .filter(|m| !m.is_deleted && m.created_at < cutoff)
            .filter(|m| {
                m.importance < self.config.integration_preserve_importance
                    && !m
                        .tags
                        .iter()
                        .any(|t| self.config.integration_preserve_tags.contains(t))
                    && m.memory_type != MemoryType::Preference
            })
            .collect();

        // P5: 排除已被整合的源记忆；上次崩溃残留的已整合源记忆
        // （写整合记忆后、删旧记忆前中断）补删，保证幂等不重复整合。
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
            for id in leftover {
                if let Err(e) = self.storage.delete(&id).await {
                    tracing::warn!("Failed to delete leftover integrated memory {}: {}", id, e);
                }
            }
        }

        // P1/P3: 按重要性升序（优先整合低价值记忆），截取单次整合上限
        candidates.sort_by(|a, b| {
            a.importance
                .partial_cmp(&b.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(self.config.integration_max_memories);

        if candidates.len() < self.config.min_memories {
            warn!(
                "Not enough memories for Dream integration: {} < {}",
                candidates.len(),
                self.config.min_memories
            );
            return Ok(DreamResult {
                integrated_count: 0,
                created_memory_id: None,
                summary: "Not enough old memories for Dream integration".to_string(),
                phase_results: Vec::new(),
            });
        }

        info!(
            "Dream processing {} memories older than {}",
            candidates.len(),
            cutoff_str
        );

        // P6: 先 LLM 整合（失败则直接返回，phases 不执行，
        // 避免 LLM 故障时阶段副作用已生效但整合未完成的状态不一致）。
        let summary = self.summarize_memories(&candidates).await?;

        // P5: 整合记忆 metadata 记录源记忆 id，供下次执行幂等去重与追溯。
        let source_ids: Vec<String> = candidates.iter().map(|m| m.id.to_string()).collect();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "source_ids".to_string(),
            serde_json::to_string(&source_ids).unwrap_or_default(),
        );

        let memory_type = match self.config.integration_type.as_str() {
            "decision" => MemoryType::Decision,
            "knowledge" => MemoryType::Knowledge,
            "context" => MemoryType::Context,
            "preference" => MemoryType::Preference,
            _ => MemoryType::Knowledge,
        };

        let integrated_memory = Memory {
            id: Uuid::new_v4(),
            project_id: Some(Uuid::nil()),
            content: summary.clone(),
            memory_type,
            tags: self.config.integration_tags.clone(),
            importance: 0.8,
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
        };

        self.storage
            .add(&integrated_memory)
            .await
            .map_err(|e| DreamError::Storage(e.to_string()))?;
        let created_id = integrated_memory.id;

        // 仅软删本次整合的候选记忆（高价值记忆已在筛选中保留）
        for memory in &candidates {
            self.storage
                .delete(&memory.id)
                .await
                .map_err(|e| DreamError::Storage(e.to_string()))?;
        }

        // 整合成功后执行阶段 1-4（LLM 失败时全部跳过）
        let mut phase_results = Vec::new();

        if self.config.phase_cross_project {
            let extractor = CrossProjectExtractor::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            let result = extractor.execute().await;
            phase_results.push(result);
        }

        if self.config.phase_personal_summary {
            let summarizer = PersonalSummarizer::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            let result = summarizer.execute().await;
            phase_results.push(result);
        }

        if self.config.phase_cleanup {
            // P9: cleanup 参数配置化（inactive_days / importance_threshold）
            let cleaner = MemoryCleaner::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
                self.config.cleanup_inactive_days,
                self.config.cleanup_importance_threshold,
            );
            let result = cleaner.execute().await;
            phase_results.push(result);
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
                let result = regenerator.execute().await;
                phase_results.push(result);
            }
        }

        info!(
            "Dream completed: {} memories integrated into {}",
            candidates.len(),
            created_id
        );

        Ok(DreamResult {
            integrated_count: candidates.len(),
            created_memory_id: Some(created_id),
            summary,
            phase_results,
        })
    }

    /// 收集所有已写入整合记忆（`dream-integrated` 标签）记录的源记忆 id。
    ///
    /// 幂等机制：整合记忆写入时把源记忆 id 列表存入 `metadata.source_ids`，
    /// 下次执行据此排除已整合的源记忆，崩溃残留（写后未删）也可识别补删。
    async fn collect_integrated_source_ids(&self) -> Result<HashSet<Uuid>, DreamError> {
        let mut seen = HashSet::new();
        let summaries = self
            .storage
            .list_by_tag("dream-integrated", 1000)
            .await
            .map_err(|e| DreamError::Storage(e.to_string()))?;
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

    async fn summarize_memories(&self, memories: &[Memory]) -> Result<String, DreamError> {
        let content_lines: Vec<String> = memories
            .iter()
            .map(|m| {
                format!(
                    "- [{}] ({}): {}",
                    m.memory_type,
                    m.created_at.format("%Y-%m-%d"),
                    m.content
                )
            })
            .collect();

        let user_prompt = format!("以下是待整合的历史记忆：\n\n{}", content_lines.join("\n"));

        let messages = vec![
            LlmMessage::system(SUMMARY_SYSTEM_PROMPT),
            LlmMessage::user(user_prompt),
        ];

        match &self.llm {
            Some(llm) => {
                info!(
                    target: "dream",
                    "Dream LLM summarization: {} memories, prompt {} chars",
                    memories.len(),
                    messages.iter().map(|m| m.content.len()).sum::<usize>()
                );
                let raw = llm
                    .chat(&messages)
                    .await
                    .map_err(|e| DreamError::Llm(e.to_string()))?;
                Ok(clean_summary(&raw))
            }
            None => {
                // 未接入 LLM 且 requires_llm = false：退化为模板摘要（兼容旧行为）
                warn!(
                    target: "dream",
                    "Dream LLM not configured, using template summary"
                );
                Ok(format!(
                    "[Dream 整合摘要] 整合了 {} 条记忆，涵盖 {} 到 {} 期间的内容。",
                    memories.len(),
                    memories
                        .iter()
                        .map(|m| m.created_at)
                        .min()
                        .unwrap_or_else(Utc::now),
                    memories
                        .iter()
                        .map(|m| m.created_at)
                        .max()
                        .unwrap_or_else(Utc::now),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

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
        assert!(matches!(result, Err(DreamError::GateFailed(_))));
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
            let mut memory = Memory::new(format!("memory {}", i), MemoryType::Knowledge);
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
            integrated_count: 5,
            created_memory_id: Some(Uuid::nil()),
            summary: "test summary".to_string(),
            phase_results: vec![],
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

        let llm = MockLlmClient::new("LLM 整合摘要内容");

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
            let mut memory = Memory::new(format!("memory {}", i), MemoryType::Knowledge);
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
            let mut memory = Memory::new(format!("memory {}", i), MemoryType::Knowledge);
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
            let mut memory = Memory::new(format!("memory {}", i), MemoryType::Knowledge);
            memory.created_at = Utc::now() - chrono::Duration::hours(200);
            memory.importance = 0.5;
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 2);
    }
}
