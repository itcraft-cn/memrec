//! # 记忆存储（分组子模块）
//!
//! 原单文件约 880 行，现拆为 crud（MemoryStorage 实现）、
//! migration（二级索引迁移/自愈）与单元测试。

use super::rocksdb::RocksDBStore;
use super::traits::{MemoryStorage, TextProvider};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use mr_common::{Memory, MemoryScope, MemoryType};
use std::collections::HashMap;
use tracing::info;
use uuid::Uuid;

/// 基于 RocksDB 的记忆存储实现。
pub struct MemoryStore {
    rocksdb: std::sync::Arc<RocksDBStore>,
}

impl MemoryStore {
    /// 创建记忆存储实例。
    pub fn new(rocksdb: std::sync::Arc<RocksDBStore>) -> Self {
        Self { rocksdb }
    }

    /// 生成记忆主键（UUID 字符串的字节表示）。
    pub(super) fn memory_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }

    /// 生成标签索引键（`tag:uuid` 格式）。
    pub(super) fn tag_key(tag: &str, id: &Uuid) -> Vec<u8> {
        format!("{}:{}", tag, id).into_bytes()
    }

    /// JSON 序列化记忆。
    pub(super) fn serialize_memory(memory: &Memory) -> Result<Vec<u8>> {
        serde_json::to_vec(memory).context("Failed to serialize memory")
    }

    /// JSON 反序列化记忆。
    pub(super) fn deserialize_memory(data: &[u8]) -> Result<Memory> {
        serde_json::from_slice(data).context("Failed to deserialize memory")
    }

    /// 项目二级索引桶：None 与 nil 项目统一为 16 字节全零（全局桶），
    /// 保证"全局"两种历史语义在索引层一致。
    pub(super) fn project_bucket(project_id: Option<&Uuid>) -> [u8; 16] {
        match project_id {
            Some(id) if !id.is_nil() => *id.as_bytes(),
            _ => [0u8; 16],
        }
    }

    /// 项目二级索引 key：桶（16B）+ 记忆 id（16B），桶内以 id 保证唯一。
    pub(super) fn project_index_key(project_id: Option<&Uuid>, id: &Uuid) -> Vec<u8> {
        let mut key = Vec::with_capacity(32);
        key.extend_from_slice(&Self::project_bucket(project_id));
        key.extend_from_slice(id.as_bytes());
        key
    }

    /// 创建时间二级索引 key：unix 秒大端（保证全库按时间递增）+ 记忆 id（16B）。
    ///
    /// created_at 校准为非负秒：负时间戳（1970 前）钳为 0，同时靠桶内 id 保证多
    /// 条同秒记录 key 唯一。秒级精度下区间边界允许少差，调用侧需对边界做二次过滤。
    pub(super) fn created_index_key(created_at: &chrono::DateTime<Utc>, id: &Uuid) -> Vec<u8> {
        let secs = created_at.timestamp().max(0) as u64;
        let mut key = Vec::with_capacity(24);
        key.extend_from_slice(&secs.to_be_bytes());
        key.extend_from_slice(id.as_bytes());
        key
    }

    /// 从迭代器中收集记忆，支持跳过已删除项。
    ///
    /// 返回 `Result`：迭代错误（I/O、文件损坏）不能当成「正常结束」，
    /// 否则调用方会拿到**成功但不完整**的结果（旧实现只判 `valid()`，
    /// 且反序列化失败项被静默跳过，双重丢失）。
    pub(super) fn collect_memories_from_iter(
        mut iter: rocksdb::DBRawIterator<'_>,
        limit: usize,
        filter_deleted: bool,
    ) -> anyhow::Result<Vec<Memory>> {
        let mut memories = Vec::new();
        let mut count = 0;

        let mut skipped = 0usize;
        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(value) = iter.value() {
                match Self::deserialize_memory(value) {
                    Ok(memory) => {
                        if !filter_deleted || !memory.is_deleted {
                            memories.push(memory);
                            count += 1;
                        }
                    }
                    Err(e) => {
                        skipped += 1;
                        tracing::warn!(target: "storage", "记忆反序列化失败，已跳过：{e}");
                    }
                }
            }
            iter.next();
        }
        iter.status()
            .map_err(|e| anyhow::anyhow!("RocksDB 迭代失败（结果不完整）：{e}"))?;
        if skipped > 0 {
            tracing::warn!(target: "storage", "本轮列表跳过 {skipped} 条无法解析的记忆");
        }

        Ok(memories)
    }
}

#[async_trait]
impl TextProvider for MemoryStore {
    /// 批量取记忆全文（调用侧候选池 ≤50，逐条 get 成本可忽略）。
    async fn texts(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, String>> {
        let mut map = HashMap::with_capacity(ids.len());
        for id in ids {
            if let Some(memory) = self.get(id).await? {
                map.insert(*id, memory.content);
            }
        }
        Ok(map)
    }
}

mod crud;
mod migration;

#[cfg(test)]
mod tests;
