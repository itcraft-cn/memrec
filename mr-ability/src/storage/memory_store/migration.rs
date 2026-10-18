//! 二级索引全量迁移与自愈。

use super::*;

impl MemoryStore {
    /// 项目桶前缀扫描的共享实现：桶 key 恰为 id，逐 id 回表并过滤软删。
    pub(super) async fn list_by_project_bucket(&self, bucket: &[u8; 16], limit: usize) -> Result<Vec<Memory>> {
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
    pub(super) async fn list_by_created_bucket(
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
