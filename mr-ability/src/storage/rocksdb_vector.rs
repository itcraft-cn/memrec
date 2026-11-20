//! # RocksDB 持久化向量存储
//!
//! [`RocksDBVectorStore`] 使用 RocksDB 持久化嵌入向量，同时维护内存缓存
//! 用于高速搜索。写入时仅更新缓存并标记 dirty，定时或关闭时批量刷盘。
//!
//! ## 写入策略
//!
//! - `add()` → 更新内存缓存 + 标记 dirty
//! - `save()` → 仅在 dirty 时执行 WriteBatch 批量写入
//! - 守护进程每 30 秒调用 `save()` 同步
//!
//! ## 搜索
//!
//! 使用暴力余弦相似度搜索，支持项目 ID、记忆类型和最低分数过滤。

use super::traits::{SearchFilter, SearchHit, VectorPayload, VectorStorage};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rocksdb::{Options, WriteBatch, DB};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// 向量列族名
const CF_VECTORS: &str = "vectors";
/// 载荷列族名
const CF_PAYLOADS: &str = "payloads";

/// RocksDB 中存储的向量记录，包含嵌入向量和载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredVector {
    embedding: Vec<f32>,
    payload: VectorPayload,
}

/// 基于 RocksDB 的持久化向量存储。
///
/// 启动时从 RocksDB 加载全部向量到内存缓存，搜索在内存中执行，
/// 写入先更新缓存，定时批量持久化。
pub struct RocksDBVectorStore {
    db: Arc<DB>,
    dimension: usize,
    cache: Arc<Mutex<HashMap<Uuid, StoredVector>>>,
    /// 待刷盘的记忆 id 集合（只记增量，避免每次脏刷新全量重写）。
    pending: Arc<Mutex<std::collections::HashSet<Uuid>>>,
}

impl RocksDBVectorStore {
    /// 打开 RocksDB 向量存储，加载已有数据到内存缓存。
    pub fn open(path: &std::path::Path, dimension: usize) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cf_vectors = rocksdb::ColumnFamilyDescriptor::new(CF_VECTORS, Options::default());
        let cf_payloads = rocksdb::ColumnFamilyDescriptor::new(CF_PAYLOADS, Options::default());

        let db = DB::open_cf_descriptors(&opts, path, vec![cf_vectors, cf_payloads])?;

        let cache = Arc::new(Mutex::new(HashMap::new()));
        let db = Arc::new(db);

        {
            let cf = db.cf_handle(CF_VECTORS).context("CF_VECTORS not found")?;
            let mut cache_lock = cache.lock().unwrap();

            // 坏记录必须留痕：静默丢弃会让对应记忆永久不可被语义检索，
            // 且启动日志无任何线索，只能靠「搜不到」被动发现。
            let mut skipped = 0usize;
            for item in db.iterator_cf(&cf, rocksdb::IteratorMode::Start) {
                let (key, value) = match item {
                    Ok(kv) => kv,
                    Err(e) => {
                        skipped += 1;
                        tracing::warn!(target: "vector", "向量迭代失败，已跳过：{e}");
                        continue;
                    }
                };
                let id = std::str::from_utf8(&key)
                    .ok()
                    .and_then(|s| Uuid::parse_str(s).ok());
                let stored = serde_json::from_slice::<StoredVector>(&value).ok();
                match (id, stored) {
                    (Some(id), Some(stored)) => {
                        if stored.embedding.len() == dimension {
                            cache_lock.insert(id, stored);
                        } else {
                            // 维度不一致的存量向量不得进入缓存：否则余弦会用 zip
                            // 静默截断成较短长度，产出貌似合理的错误相关性。
                            skipped += 1;
                            tracing::warn!(
                                target: "vector",
                                "向量维度不一致（记录 {} 维 / 期望 {} 维），已跳过 {}",
                                stored.embedding.len(),
                                dimension,
                                id
                            );
                        }
                    }
                    _ => {
                        skipped += 1;
                        tracing::warn!(
                            target: "vector",
                            "向量记录损坏（key 非 UUID 或内容无法解析），已跳过"
                        );
                    }
                }
            }
            if skipped > 0 {
                tracing::warn!(target: "vector", "启动加载向量时跳过 {} 条异常记录", skipped);
            }
        }

        Ok(Self {
            db,
            dimension,
            cache,
            pending: Arc::new(Mutex::new(std::collections::HashSet::new())),
        })
    }

    /// 将待写数据增量刷盘。
    ///
    /// 只写自上次刷盘以来变更过的 id（`pending` 集合），而不是整个缓存：
    /// 旧实现用一个 `dirty: bool`，任何一条 add 都会触发全量序列化 + 整库重写
    /// （1 万条 1024 维向量约 200MB 写放大），且序列化与 `db.write` 全程持有
    /// cache 锁，检索与写入被一起阻塞。现在序列化在锁内、写盘在锁外，
    /// 写入量与实际变更成正比。
    pub fn save(&self) -> Result<()> {
        let ids: Vec<Uuid> = {
            let mut pending = self.pending.lock().unwrap();
            if pending.is_empty() {
                return Ok(());
            }
            pending.drain().collect()
        };

        // 锁内只做序列化（纯 CPU，无 I/O），随后释放锁再写盘
        let entries: Vec<(Vec<u8>, Vec<u8>)> = {
            let cache = self.cache.lock().unwrap();
            let mut entries = Vec::with_capacity(ids.len());
            for id in &ids {
                // 期间被删除的 id 跳过（remove 已同步删除磁盘记录）
                if let Some(stored) = cache.get(id) {
                    entries.push((id.to_string().into_bytes(), serde_json::to_vec(stored)?));
                }
            }
            entries
        };

        if entries.is_empty() {
            return Ok(());
        }

        let cf_vectors = self
            .db
            .cf_handle(CF_VECTORS)
            .context("CF_VECTORS not found")?;
        let mut batch = WriteBatch::default();
        for (key, value) in &entries {
            batch.put_cf(&cf_vectors, key, value);
        }
        self.db.write(batch)?;

        Ok(())
    }

    /// 计算余弦相似度。
    ///
    /// 长度不一致时返回 0.0 而不是用 `zip` 静默截断：截断会把两个不同空间
    /// 的向量前 N 维当成同一空间计算，产出看似合理的错误相关性（换模型后
    /// 的旧向量正是这种情形）。调用方负责在加载/写入阶段拦截维度不一致。
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            tracing::warn!(
                target: "vector",
                "余弦计算维度不一致（{} vs {}），返回 0",
                a.len(),
                b.len()
            );
            return 0.0;
        }
        let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
        let norm_a = (a.iter().map(|x| x * x).sum::<f32>()).sqrt();
        let norm_b = (b.iter().map(|x| x * x).sum::<f32>()).sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    /// 返回内存缓存中的向量数量。
    pub fn count_cached(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// 是否有待刷盘的脏数据。
    ///
    /// 供定时同步循环判断是否需要实际写入，避免无变更时产生无意义日志。
    pub fn is_dirty(&self) -> bool {
        !self.pending.lock().unwrap().is_empty()
    }
}

#[async_trait]
impl VectorStorage for RocksDBVectorStore {
    async fn add(&self, id: &Uuid, embedding: &[f32], payload: VectorPayload) -> Result<()> {
        if embedding.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            ));
        }

        let stored = StoredVector {
            embedding: embedding.to_vec(),
            payload,
        };

        self.cache.lock().unwrap().insert(*id, stored);
        self.pending.lock().unwrap().insert(*id);

        Ok(())
    }

    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let existed = self.cache.lock().unwrap().remove(id).is_some();
        if existed {
            // 直接从待写集合摘除，避免刷盘时又写回已删除的 id
            self.pending.lock().unwrap().remove(id);

            let cf_vectors = self
                .db
                .cf_handle(CF_VECTORS)
                .context("CF_VECTORS not found")?;
            self.db.delete_cf(&cf_vectors, id.to_string().as_bytes())?;
        }
        Ok(existed)
    }

    async fn search(
        &self,
        query: &[f32],
        filter: SearchFilter,
        top_k: usize,
    ) -> Result<Vec<SearchHit>> {
        if query.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Query dimension mismatch: expected {}, got {}",
                self.dimension,
                query.len()
            ));
        }

        // 第一段：持锁仅完成相似度计算与过滤，不做排序（M-5：排序移出锁外，缩短持锁范围，
        // 降低对并发写入的阻塞）。
        let mut scored: Vec<(Uuid, f32)> = {
            let cache = self.cache.lock().unwrap();
            cache
                .iter()
                .filter_map(|(id, stored)| {
                    // T4: global_only 直接命中全局桶（None；历史 Some(nil) 等价处理）。
                    if filter.global_only {
                        if !super::traits::is_global_project(&stored.payload.project_id) {
                            return None;
                        }
                    } else if filter.project_id.is_some()
                        && stored.payload.project_id != filter.project_id
                        && (!filter.include_global
                            || !super::traits::is_global_project(&stored.payload.project_id))
                    {
                        return None;
                    }

                    if filter.memory_type.is_some()
                        && Some(stored.payload.memory_type.as_str())
                            != filter.memory_type.as_deref()
                    {
                        return None;
                    }

                    let sim = Self::cosine_similarity(query, &stored.embedding);
                    if sim < filter.min_score {
                        return None;
                    }

                    Some((*id, sim))
                })
                .collect()
        };

        // 分数降序；同分按 id 升序，保证多次运行结果确定。
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(top_k);

        // 第二段：短暂持锁回填 payload。持锁窗口极小；期间若发生并发移除，缺失项跳过
        // （最终一致，可接受的窄竞态）。
        let cache = self.cache.lock().unwrap();
        let hits = scored
            .into_iter()
            .filter_map(|(id, score)| {
                cache.get(&id).map(|stored| SearchHit {
                    memory_id: id,
                    score,
                    payload: stored.payload.clone(),
                    vector_score: Some(score),
                    fts_score: None,
                    coverage: None,
                    rank_score: None,
                })
            })
            .collect();

        Ok(hits)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>> {
        Ok(self
            .cache
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.embedding.clone()))
    }

    async fn count(&self) -> Result<usize> {
        Ok(self.cache.lock().unwrap().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_rocksdb_vector_store() {
        let dir = tempdir().unwrap();
        let id = Uuid::new_v4();
        let embedding = vec![1.0, 2.0, 3.0];
        let payload = VectorPayload {
            content_preview: "test".to_string(),
            ..Default::default()
        };

        {
            let store = RocksDBVectorStore::open(dir.path(), 3).unwrap();
            store.add(&id, &embedding, payload).await.unwrap();

            let retrieved = store.get(&id).await.unwrap();
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap(), embedding);

            store.save().unwrap();
        }

        let store2 = RocksDBVectorStore::open(dir.path(), 3).unwrap();
        let retrieved2 = store2.get(&id).await.unwrap();
        assert!(retrieved2.is_some());
        assert_eq!(retrieved2.unwrap(), embedding);
    }
}

#[cfg(test)]
mod incremental_save_tests {
    use super::*;

    /// 刷盘只写自上次刷盘以来变更的 id，而不是全量缓存。
    ///
    /// 回归背景：旧实现用单个 `dirty: bool`，任意一条 add 都触发整个缓存
    /// 重写，写放大约等于库体积。
    #[tokio::test]
    async fn test_save_writes_only_pending_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = RocksDBVectorStore::open(dir.path(), 3).unwrap();

        let id1 = Uuid::new_v4();
        store
            .add(&id1, &[1.0, 0.0, 0.0], VectorPayload::default())
            .await
            .unwrap();
        store.save().unwrap();
        assert!(!store.is_dirty(), "刷盘后不应再有待写数据");

        // 第二次只新增一条：待写集合应恰好只有它
        let id2 = Uuid::new_v4();
        store
            .add(&id2, &[0.0, 1.0, 0.0], VectorPayload::default())
            .await
            .unwrap();
        assert!(store.is_dirty());
        {
            let pending = store.pending.lock().unwrap();
            assert_eq!(pending.len(), 1, "只应记录增量 id");
            assert!(pending.contains(&id2));
        }
        store.save().unwrap();
        assert!(!store.is_dirty());
    }

    /// 删除后刷盘不得把已删向量写回。
    #[tokio::test]
    async fn test_remove_before_save_leaves_no_vector() {
        let dir = tempfile::tempdir().unwrap();
        let store = RocksDBVectorStore::open(dir.path(), 3).unwrap();

        let id = Uuid::new_v4();
        store
            .add(&id, &[1.0, 0.0, 0.0], VectorPayload::default())
            .await
            .unwrap();
        assert!(store.remove(&id).await.unwrap(), "应先命中缓存");
        store.save().unwrap();

        // 重新打开：磁盘上不应存在该向量
        drop(store);
        let reopened = RocksDBVectorStore::open(dir.path(), 3).unwrap();
        assert_eq!(reopened.count_cached(), 0, "已删除向量不得被写回");
    }

    /// 维度不一致的存量向量不得进入缓存（避免余弦静默截断）。
    #[test]
    fn test_open_skips_dimension_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = RocksDBVectorStore::open(dir.path(), 4).unwrap();
            // 直接写一条 3 维记录，模拟换模型前的旧数据
            let stored = StoredVector {
                embedding: vec![1.0, 0.0, 0.0],
                payload: VectorPayload::default(),
            };
            let cf = store.db.cf_handle(CF_VECTORS).unwrap();
            store
                .db
                .put_cf(
                    &cf,
                    Uuid::new_v4().to_string().as_bytes(),
                    serde_json::to_vec(&stored).unwrap(),
                )
                .unwrap();
        }

        let reopened = RocksDBVectorStore::open(dir.path(), 4).unwrap();
        assert_eq!(reopened.count_cached(), 0, "维度不一致的向量应被跳过");
    }
}
