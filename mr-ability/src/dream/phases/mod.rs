//! # Dream 阶段处理模块
//!
//! 包含4个独立的阶段处理器：
//! 1. CrossProjectExtract: 交叉项目记忆提取
//! 2. PersonalSummary: 个人总体记忆提炼
//! 3. Cleanup: 记忆清理
//! 4. VectorRegen: 向量重新生成

mod cleanup;
mod cross_project;
mod personal_summary;
mod vector_regen;

use std::sync::Arc;

use chrono::Utc;
use mr_common::types::{Memory, MemoryScope, MemorySource, MemoryType};

use crate::storage::MemoryStorage;

pub use cleanup::MemoryCleaner;
pub use cross_project::CrossProjectExtractor;
pub use personal_summary::PersonalSummarizer;
pub use vector_regen::VectorRegenerator;

/// 按标签 upsert 一条系统摘要记忆。
///
/// - 已存在同标签记忆：更新内容（保留最新一条，删除多余旧条目）
/// - 不存在：新增
///
/// 摘要记忆 source 固定为 `System`（降低检索来源权重），importance 由调用方指定。
///
/// 返回 `(新增数, 删除的多余旧条目数)`。
pub async fn upsert_summary(
    storage: &Arc<dyn MemoryStorage>,
    tag: &str,
    content: String,
    importance: f32,
) -> anyhow::Result<(usize, usize)> {
    let mut existing = storage.list_by_tag(tag, 100).await?;
    // 新 → 旧排序
    existing.sort_by_key(|b| std::cmp::Reverse(b.created_at));

    // 删除多余的旧条目，仅保留最新一条
    let mut deleted = 0;
    if existing.len() > 1 {
        for old in existing.drain(1..) {
            if storage.delete(&old.id).await? {
                deleted += 1;
            }
        }
    }

    if let Some(latest) = existing.first() {
        // 内容无变化则不写入，避免无谓的 RocksDB 写放大
        if latest.content != content {
            let mut updated = latest.clone();
            updated.content = content;
            updated.importance = importance;
            updated.source = MemorySource::System;
            updated.last_accessed = Utc::now();
            storage.update(&updated).await?;
        }
        Ok((0, deleted))
    } else {
        let mut memory = Memory::new(content, MemoryType::Knowledge)
            .with_tags(vec![tag.to_string()])
            .with_scope(MemoryScope::Global)
            .with_source(MemorySource::System);
        memory.importance = importance;
        storage.add(&memory).await?;
        Ok((1, deleted))
    }
}

/// 阶段执行结果。
#[derive(Debug, Clone)]
pub struct PhaseResult {
    pub phase: String,
    pub success: bool,
    pub processed_count: usize,
    pub created_count: usize,
    pub error_message: Option<String>,
}

impl PhaseResult {
    pub fn ok(phase: &str, processed: usize, created: usize) -> Self {
        Self {
            phase: phase.to_string(),
            success: true,
            processed_count: processed,
            created_count: created,
            error_message: None,
        }
    }

    pub fn err(phase: &str, message: String) -> Self {
        Self {
            phase: phase.to_string(),
            success: false,
            processed_count: 0,
            created_count: 0,
            error_message: Some(message),
        }
    }
}
