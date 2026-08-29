//! # DreamProcessor 记忆整合处理器
//!
//! 执行记忆压缩和整合，类似人类睡眠中的记忆巩固过程。
//!
//! ## 流程
//!
//! 1. 筛选符合条件的旧记忆
//! 2. 调用 LLM 整合生成摘要
//! 3. 写入新记忆，删除旧记忆

use std::sync::Arc;

use chrono::Utc;
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
        // 当前记忆总数：Gate 新记忆门槛与锁/状态记录均基于该值
        let memory_count = self
            .storage
            .count()
            .await
            .map_err(|e| DreamError::Storage(e.to_string()))?;

        if !force {
            // 时间门槛基于持久化的 dream.state（上次执行完成时间），
            // 与并发锁分离：release 锁后时间信息不丢失，24h 门槛不会失效。
            let last_state = self.lock.read_state();
            let gate_result = DreamGate::check(&self.config, last_state.as_ref(), memory_count);

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

        // 记录上次执行后的记忆总数，供 Gate 新记忆门槛检查。
        // 使用执行后计数：Dream 会软删/硬删旧记忆，若记录执行前计数，
        // 下次门槛会错误要求补齐被 Dream 清理的缺口。
        let post_count = self
            .storage
            .count()
            .await
            .map_err(|e| DreamError::Storage(e.to_string()))?;
        // 写失败仅告警，不影响主流程。
        if let Err(e) = self.lock.write_state(post_count as u32) {
            tracing::warn!("Failed to write dream state: {}", e);
        }

        self.lock.release()?;

        result
    }

    async fn process_inner(&self) -> Result<DreamResult, DreamError> {
        // 先筛选符合条件的旧记忆；数量不足时直接返回，
        // 不执行任何阶段（避免无意义的统计摘要写入，防止垃圾记忆堆积）。
        let cutoff = Utc::now() - chrono::Duration::hours(self.config.max_age_hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let memories = self
            .storage
            .list_older_than(&cutoff_str, 1000)
            .await
            .map_err(|e| DreamError::Storage(e.to_string()))?;

        if memories.len() < self.config.min_memories {
            warn!(
                "Not enough memories for Dream integration: {} < {}",
                memories.len(),
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
            memories.len(),
            cutoff_str
        );

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
            let cleaner = MemoryCleaner::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
                90,
                0.1,
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

        // 旧记忆已在阶段前筛选，这里直接进行 LLM 整合
        let summary = self.summarize_memories(&memories).await?;

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
            metadata: std::collections::HashMap::new(),
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

        for memory in &memories {
            self.storage
                .delete(&memory.id)
                .await
                .map_err(|e| DreamError::Storage(e.to_string()))?;
        }

        info!(
            "Dream completed: {} memories integrated into {}",
            memories.len(),
            created_id
        );

        Ok(DreamResult {
            integrated_count: memories.len(),
            created_memory_id: Some(created_id),
            summary,
            phase_results,
        })
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
            // 记忆年龄须大于 max_age_hours（100h），才会被 list_older_than 选中
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
}
