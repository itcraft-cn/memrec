//! MemoryStorage trait 实现：CRUD、查询、软删/恢复。

use super::*;

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
