//! # DreamRun 审计存储实现
//!
//! [`DreamRunStore`] 基于 [`RocksDBStore`] 实现 [`DreamRunStorage`] trait，
//! 提供 DreamRun 审计记录的落库与查询。
//!
//! ## 存储结构
//!
//! - **dream_runs 列族**：`run_id` → JSON 序列化的 [`DreamRun`]
//!
//! 查询按 `started_at` 倒序（最近优先），支持 `limit`/`offset` 分页。

use super::rocksdb::RocksDBStore;
use super::traits::DreamRunStorage;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mr_common::DreamRun;
use std::sync::Arc;
use uuid::Uuid;

/// 基于 RocksDB 的 DreamRun 审计存储实现。
pub struct DreamRunStore {
    rocksdb: Arc<RocksDBStore>,
}

impl DreamRunStore {
    /// 创建 DreamRun 存储实例（与 MemoryStore/SceneStore 共享同一 `Arc<RocksDBStore>`）。
    pub fn new(rocksdb: Arc<RocksDBStore>) -> Self {
        Self { rocksdb }
    }

    fn run_key(run_id: &Uuid) -> Vec<u8> {
        run_id.to_string().into_bytes()
    }

    fn serialize_run(run: &DreamRun) -> Result<Vec<u8>> {
        serde_json::to_vec(run).context("Failed to serialize DreamRun")
    }

    fn deserialize_run(data: &[u8]) -> Result<DreamRun> {
        serde_json::from_slice(data).context("Failed to deserialize DreamRun")
    }

    /// 全量读取（未排序）。
    fn collect_all(&self) -> Result<Vec<DreamRun>> {
        let cf = self.rocksdb.cf_dream_runs()?;
        let mut iter = self.rocksdb.iter_cf(cf);
        let mut runs = Vec::new();
        iter.seek_to_first();
        while iter.valid() {
            if let Some(value) = iter.value() {
                if let Ok(run) = Self::deserialize_run(value) {
                    runs.push(run);
                }
            }
            iter.next();
        }
        Ok(runs)
    }
}

#[async_trait]
impl DreamRunStorage for DreamRunStore {
    async fn save_run(&self, run: &DreamRun) -> Result<()> {
        let key = Self::run_key(&run.run_id);
        let data = Self::serialize_run(run)?;
        let cf = self.rocksdb.cf_dream_runs()?;
        self.rocksdb.put_cf(cf, &key, &data)?;
        Ok(())
    }

    async fn get_run(&self, run_id: &Uuid) -> Result<Option<DreamRun>> {
        let key = Self::run_key(run_id);
        let cf = self.rocksdb.cf_dream_runs()?;
        match self.rocksdb.get_cf(cf, &key)? {
            Some(bytes) => Ok(Some(Self::deserialize_run(&bytes)?)),
            None => Ok(None),
        }
    }

    async fn list_runs(&self, limit: usize, offset: usize) -> Result<Vec<DreamRun>> {
        let mut runs = self.collect_all()?;
        // 最近优先
        runs.sort_by_key(|r| std::cmp::Reverse(r.started_at));
        Ok(runs.into_iter().skip(offset).take(limit).collect())
    }

    async fn count_runs(&self) -> Result<usize> {
        let cf = self.rocksdb.cf_dream_runs()?;
        Ok(self.rocksdb.count_cf_keys(cf)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_common::{DreamRun, DreamRunStatus};
    use tempfile::tempdir;

    fn make_store() -> (DreamRunStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let rocksdb = Arc::new(RocksDBStore::open(dir.path()).unwrap());
        (DreamRunStore::new(rocksdb), dir)
    }

    #[tokio::test]
    async fn test_save_and_get_run() {
        let (store, _dir) = make_store();
        let mut run = DreamRun::running(true, false);
        run.finish(DreamRunStatus::Success);

        store.save_run(&run).await.unwrap();
        let got = store.get_run(&run.run_id).await.unwrap().unwrap();
        assert_eq!(got.run_id, run.run_id);
        assert_eq!(got.status, DreamRunStatus::Success);
    }

    #[tokio::test]
    async fn test_list_runs_recent_first() {
        let (store, _dir) = make_store();
        for i in 0..3 {
            let mut run = DreamRun::running(false, false);
            run.started_at = chrono::Utc::now() + chrono::Duration::seconds(i as i64);
            run.finish(DreamRunStatus::Success);
            store.save_run(&run).await.unwrap();
        }
        let runs = store.list_runs(10, 0).await.unwrap();
        assert_eq!(runs.len(), 3);
        assert!(runs[0].started_at >= runs[1].started_at);
    }

    #[tokio::test]
    async fn test_count_and_paging() {
        let (store, _dir) = make_store();
        for _ in 0..5 {
            let run = DreamRun::running(false, false);
            store.save_run(&run).await.unwrap();
        }
        assert_eq!(store.count_runs().await.unwrap(), 5);
        let page = store.list_runs(2, 1).await.unwrap();
        assert_eq!(page.len(), 2);
    }
}
