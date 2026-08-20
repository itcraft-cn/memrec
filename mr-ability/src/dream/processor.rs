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
use tokio::sync::RwLock;
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
    session_count: Arc<RwLock<u32>>,
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
            session_count: Arc::new(RwLock::new(0)),
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

    pub fn increment_session(&self) {
        let mut count = self.session_count.blocking_write();
        *count += 1;
    }

    pub async fn session_count(&self) -> u32 {
        *self.session_count.read().await
    }

    pub async fn execute(&self, force: bool) -> Result<DreamResult, DreamError> {
        if !force {
            let session_count = self.session_count.read().await;
            let last_lock = self.lock.get_lock_content();

            let gate_result = DreamGate::check(&self.config, last_lock.as_ref(), *session_count);
            drop(session_count);

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
                DreamGateResult::NotEnoughSessions { current, min } => {
                    return Err(DreamError::GateFailed(format!(
                        "Not enough sessions: {} < {}",
                        current, min
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

        let session_count = *self.session_count.read().await;
        if !self.lock.try_acquire(session_count)? {
            return Err(DreamError::GateFailed(
                "Another Dream process is running".to_string(),
            ));
        }

        let result = self.process_inner().await;

        self.lock.release()?;

        result
    }

    async fn process_inner(&self) -> Result<DreamResult, DreamError> {
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
                summary: "Phases completed, but not enough old memories for integration"
                    .to_string(),
                phase_results,
            });
        }

        info!(
            "Dream processing {} memories older than {}",
            memories.len(),
            cutoff_str
        );

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
            .map(|m| format!("- [{}] {}", m.memory_type, m.content))
            .collect();

        let system_prompt = "你是记忆库整合助手。请对给定的历史记忆进行提炼整合：\
            1. 提取关键信息\
            2. 建立关联\
            3. 去除冗余\
            4. 生成简洁摘要\
            输出要求：一段简洁的摘要文字（不超过500字）。";

        let user_prompt = format!("请整合以下历史记忆：\n\n{}", content_lines.join("\n"));

        let messages = vec![
            LlmMessage::system(system_prompt),
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
                llm.chat(&messages)
                    .await
                    .map_err(|e| DreamError::Llm(e.to_string()))
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
