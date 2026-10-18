//! # 记忆存储实现
//!
//! [`MemoryStore`] 基于 [`RocksDBStore`] 实现 [`MemoryStorage`] trait，
//! 提供记忆的完整生命周期管理。
//!
//! ## 存储结构
//!
//! - **memories 列族**：UUID → JSON 序列化的 `Memory`
//! - **by_tag 列族**：`tag:uuid` → UUID（标签索引）
//! - **deleted 列族**：UUID → UUID（软删除索引）
//! - **importance 列族**：UUID → 重要性分数字符串
//!
//! ## 软删除机制
//!
//! 首次删除时标记 `is_deleted=true` 并记录 `deleted_at`，
//! 第二次删除（硬删除）才真正从存储中移除。

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
    fn memory_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }

    /// 生成标签索引键（`tag:uuid` 格式）。
    fn tag_key(tag: &str, id: &Uuid) -> Vec<u8> {
        format!("{}:{}", tag, id).into_bytes()
    }

    /// JSON 序列化记忆。
    fn serialize_memory(memory: &Memory) -> Result<Vec<u8>> {
        serde_json::to_vec(memory).context("Failed to serialize memory")
    }

    /// JSON 反序列化记忆。
    fn deserialize_memory(data: &[u8]) -> Result<Memory> {
        serde_json::from_slice(data).context("Failed to deserialize memory")
    }

    /// 项目二级索引桶：None 与 nil 项目统一为 16 字节全零（全局桶），
    /// 保证"全局"两种历史语义在索引层一致。
    fn project_bucket(project_id: Option<&Uuid>) -> [u8; 16] {
        match project_id {
            Some(id) if !id.is_nil() => *id.as_bytes(),
            _ => [0u8; 16],
        }
    }

    /// 项目二级索引 key：桶（16B）+ 记忆 id（16B），桶内以 id 保证唯一。
    fn project_index_key(project_id: Option<&Uuid>, id: &Uuid) -> Vec<u8> {
        let mut key = Vec::with_capacity(32);
        key.extend_from_slice(&Self::project_bucket(project_id));
        key.extend_from_slice(id.as_bytes());
        key
    }

    /// 创建时间二级索引 key：unix 秒大端（保证全库按时间递增）+ 记忆 id（16B）。
    ///
    /// created_at 校准为非负秒：负时间戳（1970 前）钳为 0，同时靠桶内 id 保证多
    /// 条同秒记录 key 唯一。秒级精度下区间边界允许少差，调用侧需对边界做二次过滤。
    fn created_index_key(created_at: &chrono::DateTime<Utc>, id: &Uuid) -> Vec<u8> {
        let secs = created_at.timestamp().max(0) as u64;
        let mut key = Vec::with_capacity(24);
        key.extend_from_slice(&secs.to_be_bytes());
        key.extend_from_slice(id.as_bytes());
        key
    }

    /// 从迭代器中收集记忆，支持跳过已删除项。
    fn collect_memories_from_iter(
        mut iter: rocksdb::DBRawIterator<'_>,
        limit: usize,
        filter_deleted: bool,
    ) -> Vec<Memory> {
        let mut memories = Vec::new();
        let mut count = 0;

        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(value) = iter.value() {
                if let Ok(memory) = Self::deserialize_memory(value) {
                    if !filter_deleted || !memory.is_deleted {
                        memories.push(memory);
                        count += 1;
                    }
                }
            }
            iter.next();
        }

        memories
    }
}

#[async_trait]
impl MemoryStorage for MemoryStore {
    async fn save(&self, memory: &Memory) -> Result<()> {
        let id_key = Self::memory_key(&memory.id);
        let data = Self::serialize_memory(memory)?;

        // 写前读旧记录：清理变更后被移除的 tag / project 旧索引项，
        // 避免索引残留导致按旧标签/旧项目命中到已迁移的记忆。
        // 先 await 完成读取，再创建 WriteBatch（含裸指针，不允许跨 await 存活）。
        let old = self.get(&memory.id).await?;

        let cf_memories = self.rocksdb.cf_memories()?;
        let cf_by_tag = self.rocksdb.cf_by_tag()?;
        let cf_importance = self.rocksdb.cf_importance()?;
        let cf_by_created = self.rocksdb.cf_by_created()?;
        let cf_by_project = self.rocksdb.cf_by_project()?;

        let mut batch = rocksdb::WriteBatch::default();

        // 批内顺序约束：先执行旧索引项清理，再写入新值。WriteBatch 严格按
        // 提交顺序生效，若同 key 先 put 后 delete（如 nil 与 None 同桶迁移），
        // 收尾的 delete 会抹掉刚写入的新值。
        if let Some(old) = old {
            for tag in &old.tags {
                if !memory.tags.contains(tag) {
                    batch.delete_cf(cf_by_tag, Self::tag_key(tag, &memory.id));
                }
            }
            if old.project_id != memory.project_id {
                batch.delete_cf(
                    cf_by_project,
                    Self::project_index_key(old.project_id.as_ref(), &memory.id),
                );
            }
        }

        batch.put_cf(cf_memories, &id_key, &data);
        batch.put_cf(
            cf_importance,
            &id_key,
            memory.importance.to_string().into_bytes(),
        );
        batch.put_cf(
            cf_by_created,
            Self::created_index_key(&memory.created_at, &memory.id),
            memory.id.as_bytes(),
        );
        batch.put_cf(
            cf_by_project,
            Self::project_index_key(memory.project_id.as_ref(), &memory.id),
            memory.id.as_bytes(),
        );

        for tag in &memory.tags {
            batch.put_cf(cf_by_tag, Self::tag_key(tag, &memory.id), &id_key);
        }

        self.rocksdb.write_batch(batch)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Memory>> {
        let id_key = Self::memory_key(id);
        let cf_memories = self.rocksdb.cf_memories()?;

        match self.rocksdb.get_cf(cf_memories, &id_key)? {
            Some(bytes) => {
                let memory = Self::deserialize_memory(&bytes)?;
                Ok(Some(memory))
            }
            None => Ok(None),
        }
    }

    /// 覆写默认逐条回退：改用 `multi_get_cf` 一次性多路读取，消除 N+1 查询。
    async fn get_many(&self, ids: &[Uuid]) -> Result<Vec<Option<Memory>>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let cf_memories = self.rocksdb.cf_memories()?;
        let keys: Vec<Vec<u8>> = ids.iter().map(Self::memory_key).collect();
        let raw = self
            .rocksdb
            .multi_get_cf(cf_memories, keys.iter().map(|k| k.as_slice()));

        let mut out = Vec::with_capacity(ids.len());
        for item in raw {
            match item? {
                Some(bytes) => out.push(Some(Self::deserialize_memory(&bytes)?)),
                None => out.push(None),
            }
        }
        Ok(out)
    }

    async fn update(&self, memory: &Memory) -> Result<()> {
        self.save(memory).await
    }

    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let memory = self.get(id).await?;

        match memory {
            Some(mem) => {
                if mem.is_deleted {
                    let id_key = Self::memory_key(id);
                    let cf_memories = self.rocksdb.cf_memories()?;
                    self.rocksdb.delete_cf(cf_memories, &id_key)?;

                    let cf_deleted = self.rocksdb.cf_deleted()?;
                    self.rocksdb.delete_cf(cf_deleted, &id_key)?;

                    for tag in &mem.tags {
                        let tag_key = Self::tag_key(tag, id);
                        let cf_by_tag = self.rocksdb.cf_by_tag()?;
                        self.rocksdb.delete_cf(cf_by_tag, &tag_key)?;
                    }

                    Ok(true)
                } else {
                    let mut mem_deleted = mem;
                    mem_deleted.is_deleted = true;
                    mem_deleted.deleted_at = Some(Utc::now());
                    self.update(&mem_deleted).await?;

                    let id_key = Self::memory_key(id);
                    let cf_deleted = self.rocksdb.cf_deleted()?;
                    self.rocksdb.put_cf(cf_deleted, &id_key, &id_key)?;

                    Ok(false)
                }
            }
            None => Ok(false),
        }
    }

    async fn restore(&self, id: &Uuid) -> Result<Option<Memory>> {
        let memory = self.get(id).await?;

        match memory {
            Some(mut mem) if mem.is_deleted => {
                mem.is_deleted = false;
                mem.deleted_at = None;
                self.save(&mem).await?;

                // 清理软删除索引（与 delete 对称），保证 count() 统计一致
                let id_key = Self::memory_key(id);
                let cf_deleted = self.rocksdb.cf_deleted()?;
                self.rocksdb.delete_cf(cf_deleted, &id_key)?;

                Ok(Some(mem))
            }
            _ => Ok(None),
        }
    }

    async fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        let cf_memories = self.rocksdb.cf_memories()?;
        let iter = self.rocksdb.iter_cf(cf_memories);

        Ok(Self::collect_memories_from_iter(iter, limit, true))
    }

    async fn list_by_type(&self, memory_type: MemoryType, limit: usize) -> Result<Vec<Memory>> {
        let all = self.list(limit * 10).await?;
        Ok(all
            .into_iter()
            .filter(|m| m.memory_type == memory_type)
            .take(limit)
            .collect())
    }

    async fn list_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Memory>> {
        let cf_by_tag = self.rocksdb.cf_by_tag()?;
        let mut iter = self.rocksdb.iter_cf(cf_by_tag);

        let prefix = format!("{}:", tag).into_bytes();
        let mut memories = Vec::new();
        let mut count = 0;

        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(key) = iter.key() {
                if key.starts_with(&prefix) {
                    if let Some(value) = iter.value() {
                        let id_str = String::from_utf8_lossy(value);
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            if let Some(memory) = self.get(&id).await? {
                                if !memory.is_deleted {
                                    memories.push(memory);
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }

    async fn list_by_importance(&self, min: f32, max: f32) -> Result<Vec<Memory>> {
        let cf_importance = self.rocksdb.cf_importance()?;
        let mut iter = self.rocksdb.iter_cf(cf_importance);

        let mut memories = Vec::new();

        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                if let Some(value) = iter.value() {
                    let importance_str = String::from_utf8_lossy(value);
                    if let Ok(importance) = importance_str.parse::<f32>() {
                        if importance >= min && importance <= max {
                            let id_str = String::from_utf8_lossy(key);
                            if let Ok(id) = Uuid::parse_str(&id_str) {
                                if let Some(memory) = self.get(&id).await? {
                                    if !memory.is_deleted {
                                        memories.push(memory);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }

    async fn list_deleted(&self) -> Result<Vec<Memory>> {
        let cf_deleted = self.rocksdb.cf_deleted()?;
        let mut iter = self.rocksdb.iter_cf(cf_deleted);

        let mut memories = Vec::new();

        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                let id_str = String::from_utf8_lossy(key);
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    if let Some(memory) = self.get(&id).await? {
                        if memory.is_deleted {
                            memories.push(memory);
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }

    async fn count(&self) -> Result<usize> {
        // 未删除记忆数 = memories 列族 key 数 - deleted 列族 key 数（软删除索引）
        // 仅迭代 key 计数，不反序列化，避免每次调用全量 list
        let cf_memories = self.rocksdb.cf_memories()?;
        let cf_deleted = self.rocksdb.cf_deleted()?;
        let total = self.rocksdb.count_cf_keys(cf_memories)?;
        let deleted = self.rocksdb.count_cf_keys(cf_deleted)?;
        Ok(total.saturating_sub(deleted))
    }

    async fn count_deleted(&self) -> Result<usize> {
        let deleted = self.list_deleted().await?;
        Ok(deleted.len())
    }

    async fn list_by_project(&self, project_id: &Uuid) -> Result<Vec<Memory>> {
        // 由 by_project 列族按桶前缀定位，不再全量扫描。
        // 具体项目桶不含 None/nil 记忆，与原"project_id 精确匹配"语义一致。
        if project_id.is_nil() {
            // nil 项目桶实际不存在（None/nil 统一落全局桶），回退全量过滤保持语义。
            let all = self.list(10000).await?;
            Ok(all
                .into_iter()
                .filter(|m| m.project_id == Some(*project_id))
                .collect())
        } else {
            let memories = self
                .list_by_project_bucket(&Self::project_bucket(Some(project_id)), usize::MAX)
                .await?;
            Ok(memories
                .into_iter()
                .filter(|m| m.project_id == Some(*project_id))
                .collect())
        }
    }

    /// 按项目桶列出未删除记忆：全局桶（全零）同时覆盖 None 与 `Some(nil)`。
    async fn list_global(&self, limit: usize) -> Result<Vec<Memory>> {
        self.list_by_project_bucket(&Self::project_bucket(None), limit)
            .await
    }

    /// 按创建时间区间 `(after_unix, before_unix]` 列出未删除记忆。
    ///
    /// 秒级索引允许边界少差，此处对边界记忆做精确二次过滤，保证与
    /// `created_at > after && created_at <= before` 语义一致。
    async fn list_created_between(
        &self,
        after_unix: i64,
        before_unix: i64,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let after = chrono::DateTime::<Utc>::from_timestamp(after_unix, 0)
            .unwrap_or(chrono::DateTime::<Utc>::UNIX_EPOCH);
        let before = chrono::DateTime::<Utc>::from_timestamp(before_unix, 0)
            .unwrap_or(chrono::DateTime::<Utc>::UNIX_EPOCH);
        let memories = self
            .list_by_created_bucket(after.timestamp(), before.timestamp(), limit)
            .await?;
        Ok(memories
            .into_iter()
            .filter(|m| m.created_at > after && m.created_at <= before)
            .collect())
    }

    async fn get_chunks_by_group(&self, chunk_group_id: &Uuid) -> Result<Vec<Memory>> {
        let all = self.list(10000).await?;
        Ok(all
            .into_iter()
            .filter(|m| m.chunk_group_id == Some(*chunk_group_id))
            .collect())
    }

    async fn list_older_than(&self, cutoff: &str, limit: usize) -> Result<Vec<Memory>> {
        let cutoff_time = chrono::DateTime::parse_from_rfc3339(cutoff)
            .context("Invalid cutoff time format")?
            .with_timezone(&Utc);

        let all = self.list(limit * 10).await?;
        Ok(all
            .into_iter()
            .filter(|m| m.created_at < cutoff_time)
            .take(limit)
            .collect())
    }
}

impl MemoryStore {
    /// 项目桶前缀扫描的共享实现：桶 key 恰为 id，逐 id 回表并过滤软删。
    async fn list_by_project_bucket(&self, bucket: &[u8; 16], limit: usize) -> Result<Vec<Memory>> {
        let cf_by_project = self.rocksdb.cf_by_project()?;
        let mut iter = self.rocksdb.iter_cf(cf_by_project);
        let mut memories = Vec::new();

        iter.seek(bucket);
        while iter.valid() && memories.len() < limit {
            if let Some(key) = iter.key() {
                if key.len() < 16 || key[..16] != *bucket {
                    break;
                }
                if let Some(value) = iter.value() {
                    if let Ok(id) = Uuid::from_slice(value) {
                        if let Some(memory) = self.get(&id).await? {
                            if !memory.is_deleted {
                                memories.push(memory);
                            }
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }

    /// 创建时间区间前缀扫描的共享实现：key 前 8 字节为 unix 秒大端，天然有序。
    ///
    /// 边界取整到秒（下界含，上界含都允许），调用侧需再按精确时间过滤。
    async fn list_by_created_bucket(
        &self,
        after_secs: i64,
        before_secs: i64,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let lower = (after_secs.max(0) as u64).to_be_bytes();
        let upper = (before_secs.max(0) as u64).to_be_bytes();
        let cf_by_created = self.rocksdb.cf_by_created()?;
        let mut iter = self.rocksdb.iter_cf(cf_by_created);
        let mut memories = Vec::new();

        iter.seek(lower);
        while iter.valid() && memories.len() < limit {
            if let Some(key) = iter.key() {
                if key.len() < 24 {
                    iter.next();
                    continue;
                }
                if key[..8] > upper[..] {
                    break;
                }
                if let Some(value) = iter.value() {
                    if let Ok(id) = Uuid::from_slice(value) {
                        if let Some(memory) = self.get(&id).await? {
                            if !memory.is_deleted {
                                memories.push(memory);
                            }
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(memories)
    }
}

impl MemoryStore {
    /// 启动期语义迁移（T4）：历史记忆的 `Some(nil)` 项目归一为 None，
    /// `Workspace` scope 归一为 Global（"全局"唯一判据 = project_id None）。
    ///
    /// 活跃与软删记忆均迁移；save 会同步维护 by_created/by_project 索引。
    /// 返回迁移条数（用于日志与测试断言）。
    pub async fn migrate_global_semantics(&self) -> Result<usize> {
        let active = self.list(usize::MAX).await?;
        let deleted = self.list_deleted().await?;

        let mut migrated = 0usize;
        for m in active.into_iter().chain(deleted) {
            let mut memory = m;
            let mut changed = false;

            if memory.project_id == Some(Uuid::nil()) {
                memory.project_id = None;
                changed = true;
            }
            // T4: Workspace 已废弃，迁移中显式读取该历史值
            #[allow(deprecated)]
            if memory.scope == MemoryScope::Workspace {
                memory.scope = MemoryScope::Global;
                changed = true;
            }

            if changed {
                self.save(&memory).await?;
                migrated += 1;
            }
        }
        Ok(migrated)
    }

    /// 启动期自愈：为缺少 by_created / by_project 二级索引的记忆补写索引项。
    ///
    /// 仅在启动时全量扫描一次（与 reconcile_fts_index 同理），服务运行期索引由
    /// save 维护。返回补写索引的记忆条数（用于日志）。
    pub async fn reconcile_secondary_indexes(&self) -> Result<usize> {
        let all = self.list(usize::MAX).await?;
        let cf_by_created = self.rocksdb.cf_by_created()?;
        let cf_by_project = self.rocksdb.cf_by_project()?;

        let mut batch = rocksdb::WriteBatch::default();
        let mut fixed = 0usize;
        for m in &all {
            let created_key = Self::created_index_key(&m.created_at, &m.id);
            let project_key = Self::project_index_key(m.project_id.as_ref(), &m.id);
            let mut need = false;
            if self.rocksdb.get_cf(cf_by_created, &created_key)?.is_none() {
                batch.put_cf(cf_by_created, &created_key, m.id.as_bytes());
                need = true;
            }
            if self.rocksdb.get_cf(cf_by_project, &project_key)?.is_none() {
                batch.put_cf(cf_by_project, &project_key, m.id.as_bytes());
                need = true;
            }
            if need {
                fixed += 1;
            }
        }

        if fixed > 0 {
            self.rocksdb.write_batch(batch)?;
            info!(
                "Secondary indexes reconciled: {} memories were missing by_created/by_project entries",
                fixed
            );
        }
        Ok(fixed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_memory_save_and_get() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["tag1".to_string()]);

        store.save(&memory).await.unwrap();

        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "test content");
    }

    #[tokio::test]
    async fn test_memory_list() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        for i in 0..5 {
            let memory = Memory::new(format!("content {}", i), MemoryType::Conversation);
            store.save(&memory).await.unwrap();
        }

        let memories = store.list(10).await.unwrap();
        assert_eq!(memories.len(), 5);
    }

    /// 构造指定创建时间与项目的记忆（created_at 直接覆写，用于下推查询测试）。
    fn backdated_memory(content: &str, hours_ago: i64, project_id: Option<uuid::Uuid>) -> Memory {
        let mut memory = Memory::new(content.to_string(), MemoryType::Knowledge);
        memory.created_at = Utc::now() - chrono::Duration::hours(hours_ago);
        memory.project_id = project_id;
        memory
    }

    #[tokio::test]
    async fn test_created_between_index_pushdown() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let old = backdated_memory("old", 48, None);
        let mid = backdated_memory("mid", 26, Some(uuid::Uuid::new_v4()));
        // recent 显式回溯 2 小时，避免与 before 的秒级截断产生同秒歧义
        let recent = backdated_memory("recent", 2, None);

        store.save(&old).await.unwrap();
        store.save(&mid).await.unwrap();
        store.save(&recent).await.unwrap();

        // 区间 (30h 前, 20h 前]：只含 mid
        let after = (Utc::now() - chrono::Duration::hours(30)).timestamp();
        let before = (Utc::now() - chrono::Duration::hours(20)).timestamp();
        let result = store.list_created_between(after, before, 10).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, mid.id);

        // 下界之前：只含 old（recent 不落区间）
        let result = store.list_created_between(0, before, 10).await.unwrap();
        assert_eq!(result.iter().filter(|m| m.id == old.id).count(), 1);
        assert_eq!(result.iter().filter(|m| m.id == recent.id).count(), 0);

        // 上界之后：只含 recent
        let now = Utc::now().timestamp();
        let result = store.list_created_between(before, now, 10).await.unwrap();
        assert_eq!(result.iter().filter(|m| m.id == recent.id).count(), 1);

        // 软删除记录不进入查询结果
        store.delete(&mid.id).await.unwrap();
        let result = store.list_created_between(after, before, 10).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_project_index_pushdown_and_migration() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let pid = uuid::Uuid::new_v4();
        let proj = backdated_memory("proj", 48, Some(pid));
        let global_long_ago = backdated_memory("global", 40, None);
        // 历史 Dream 产出语义：Some(nil) 记全局桶
        let legacy = backdated_memory("legacy", 30, Some(uuid::Uuid::nil()));

        store.save(&proj).await.unwrap();
        store.save(&global_long_ago).await.unwrap();
        store.save(&legacy).await.unwrap();

        // 全局桶同时覆盖 None 与 Some(nil)
        let globals = store.list_global(10).await.unwrap();
        assert_eq!(globals.len(), 2);
        assert!(globals.iter().any(|m| m.id == global_long_ago.id));
        assert!(globals.iter().any(|m| m.id == legacy.id));

        // 具体项目桶精确命中
        let in_project = store.list_by_project(&pid).await.unwrap();
        assert_eq!(in_project.len(), 1);
        assert_eq!(in_project[0].id, proj.id);

        // 迁移：项目记忆改为全局后，旧项目索引项必须清理
        let mut migrated = proj.clone();
        migrated.project_id = None;
        store.save(&migrated).await.unwrap();
        assert!(store.list_by_project(&pid).await.unwrap().is_empty());
        let globals_after = store.list_global(10).await.unwrap();
        assert!(globals_after.iter().any(|m| m.id == migrated.id));
    }

    #[tokio::test]
    async fn test_reconcile_secondary_indexes_backfills() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        // 模拟历史数据：先写记忆库（老版本路径无二级索引），
        // save 现已维护索引，此处手工破坏一条的索引项来验证补写。
        let m = backdated_memory("legacy", 48, None);
        store.save(&m).await.unwrap();
        let rocksdb2 = store.rocksdb.clone();
        let cf_created = rocksdb2.cf_by_created().unwrap();
        let key = MemoryStore::created_index_key(&m.created_at, &m.id);
        rocksdb2.delete_cf(&cf_created, &key).unwrap();

        let fixed = store.reconcile_secondary_indexes().await.unwrap();
        assert_eq!(fixed, 1);

        let result = store
            .list_created_between(
                (Utc::now() - chrono::Duration::hours(50)).timestamp(),
                (Utc::now() - chrono::Duration::hours(20)).timestamp(),
                10,
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
    }

    /// T4 验证：Some(nil) 项目与 Workspace scope 在启动迁移中归一为 None / Global，
    /// 且活跃与软删记录一并处理，save 同步维护 by_project 索引。
    #[tokio::test]
    async fn test_migrate_global_semantics() {
        use mr_common::{MemoryScope, MemoryType};

        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let mut legacy_nil = Memory::new("legacy nil".to_string(), MemoryType::Knowledge);
        legacy_nil.project_id = Some(Uuid::nil());

        let mut legacy_ws = Memory::new("legacy workspace".to_string(), MemoryType::Knowledge);
        legacy_ws.project_id = Some(Uuid::nil());
        #[allow(deprecated)]
        {
            legacy_ws.scope = MemoryScope::Workspace;
        }

        let mut normal = Memory::new("normal".to_string(), MemoryType::Knowledge);
        normal.project_id = None;

        let mut project = Memory::new("in project".to_string(), MemoryType::Knowledge);
        project.project_id = Some(Uuid::new_v4());

        let mut deleted_nil = Memory::new("deleted nil".to_string(), MemoryType::Knowledge);
        deleted_nil.project_id = Some(Uuid::nil());

        store.save(&legacy_nil).await.unwrap();
        store.save(&legacy_ws).await.unwrap();
        store.save(&normal).await.unwrap();
        store.save(&project).await.unwrap();
        store.save(&deleted_nil).await.unwrap();
        store.delete(&deleted_nil.id).await.unwrap();

        // 3 条需迁移：legacy_nil、legacy_ws、deleted_nil（normal/project 原样）
        let migrated = store.migrate_global_semantics().await.unwrap();
        assert_eq!(migrated, 3);

        // 另一种载体验证：nil 记忆归一后进入全局桶（None ∪ nil 判据），with_project 索引同步
        let global_after = store.list_global(usize::MAX).await.unwrap();
        assert_eq!(global_after.len(), 3);
        assert!(global_after.iter().any(|m| m.id == legacy_nil.id));
        assert!(global_after.iter().any(|m| m.id == legacy_ws.id));

        // 具体项目记忆不受迁移影响
        let in_project = store
            .list_by_project(&project.project_id.unwrap())
            .await
            .unwrap();
        assert_eq!(in_project.len(), 1);

        // 软删的 nil 记忆同样迁移
        let restored = store.restore(&deleted_nil.id).await.unwrap().unwrap();
        assert_eq!(restored.project_id, None);

        // 幂等：再次迁移无变化
        let migrated_again = store.migrate_global_semantics().await.unwrap();
        assert_eq!(migrated_again, 0);
    }

    #[tokio::test]
    async fn test_memory_list_by_tag() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let m1 = Memory::new("content 1".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["rust".to_string()]);
        let m2 = Memory::new("content 2".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["python".to_string()]);

        store.save(&m1).await.unwrap();
        store.save(&m2).await.unwrap();

        let rust_memories = store.list_by_tag("rust", 10).await.unwrap();
        assert_eq!(rust_memories.len(), 1);
    }

    #[tokio::test]
    async fn test_memory_soft_delete() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let memory = Memory::new("test".to_string(), MemoryType::Conversation);
        store.save(&memory).await.unwrap();

        let deleted = store.delete(&memory.id).await.unwrap();
        assert!(!deleted);

        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert!(retrieved.unwrap().is_deleted);
    }

    #[tokio::test]
    async fn test_memory_restore_clears_deleted_index() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        store.save(&memory).await.unwrap();

        // 软删 → count 减少
        store.delete(&memory.id).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 0);
        assert_eq!(store.count_deleted().await.unwrap(), 1);

        // 恢复 → is_deleted=false 且软删索引清理，count 恢复
        let restored = store.restore(&memory.id).await.unwrap();
        assert!(restored.is_some());
        assert!(!restored.unwrap().is_deleted);
        assert_eq!(store.count().await.unwrap(), 1);
        assert_eq!(store.count_deleted().await.unwrap(), 0);

        // 再次 restore：非删除状态返回 None
        assert!(store.restore(&memory.id).await.unwrap().is_none());
    }
}
