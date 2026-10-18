//! # 索引一致型存储装饰器
//!
//! [`IndexingStorage`] 在 [`MemoryStorage`] 之上叠加一层**检索索引一致性维护**：
//! 任何 `save` / `update` / `delete` / `restore` 都会同步反映到混合检索索引
//! （向量 + 全文）。
//!
//! ## 背景
//!
//! 正常写入路径（Router 处理 `StoreMemory`）在写库后显式调用 `hybrid_store.add`
//! 维护索引；而 Dream 处理链（融合产出、软删源、阶段清理/摘要）直接操作
//! [`MemoryStorage`]，**绕过了索引维护**，导致：
//!
//! - 融合产出记忆无法被普通检索命中（仅能经图扩散捞出）；
//! - 被软删的源记忆仍作为"幽灵结果"出现在检索中。
//!
//! 该装饰器把 Dream 链路的全部存储变更统一接入索引，从而消除上述不一致。
//!
//! ## 语义
//!
//! - `save`：先写库，再重建索引（先移除旧文档再写入，保证幂等、避免重复文档）；
//! - `update`：同上（内容可能变化，须重建）；
//! - `delete`：先写库软删，再从索引移除；
//! - `restore`：先写库恢复，再重建索引；
//! - 索引操作失败仅告警、不向上传播，避免因索引侧问题中断 Dream；
//!   但写库成功即视为业务成功。
//! - 只读方法全部透传底层存储。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mr_common::{Memory, MemoryType};
use tracing::warn;
use uuid::Uuid;

use crate::embedding::EmbeddingGenerator;
use crate::storage::traits::{HybridStorage, MemoryStorage, VectorPayload};

/// 索引一致型记忆存储装饰器。
///
/// 持有底层 [`MemoryStorage`]、混合索引 [`HybridStorage`] 与嵌入生成器，
/// 将全部写操作同步到索引。
pub struct IndexingStorage {
    /// 底层记忆存储（RocksDB）
    inner: Arc<dyn MemoryStorage>,
    /// 混合检索索引（向量 + 全文）
    hybrid: Arc<dyn HybridStorage>,
    /// 嵌入生成器（索引需要向量）
    embedder: Arc<dyn EmbeddingGenerator>,
}

impl IndexingStorage {
    /// 构造装饰器。
    pub fn new(
        inner: Arc<dyn MemoryStorage>,
        hybrid: Arc<dyn HybridStorage>,
        embedder: Arc<dyn EmbeddingGenerator>,
    ) -> Self {
        Self {
            inner,
            hybrid,
            embedder,
        }
    }

    /// 由记忆构建向量索引载荷。
    fn payload(memory: &Memory) -> VectorPayload {
        VectorPayload {
            project_id: memory.project_id,
            memory_type: memory.memory_type.to_string(),
            tags: memory.tags.clone(),
            content_preview: memory.content.chars().take(200).collect(),
            importance: memory.importance,
            chunk_group_id: memory.chunk_group_id,
            chunk_index: memory.chunk_index,
            chunk_total: memory.chunk_total,
        }
    }

    /// 重建单条记忆的索引（先移除旧文档，再写入向量与全文）。
    ///
    /// 先移除是为了保证 `update` 场景下全文索引不产生重复文档；
    /// 对新记忆该移除为无副作用操作。失败仅告警。
    async fn reindex(&self, memory: &Memory) {
        // 优先复用记忆已携带的向量（如 Dream 上游去重时预计算并写回），
        // 避免重复嵌入；无预置向量时再回退到嵌入生成器。
        let embedding = match memory.embedding.clone().filter(|v| !v.is_empty()) {
            Some(v) => v,
            None => match self.embedder.embed(&memory.content) {
                Ok(v) if !v.is_empty() => v,
                Ok(_) => {
                    warn!(
                        target: "indexing",
                        "Skip indexing memory {}: empty embedding",
                        memory.id
                    );
                    return;
                }
                Err(e) => {
                    warn!(
                        target: "indexing",
                        "Skip indexing memory {}: embedding failed: {}",
                        memory.id,
                        e
                    );
                    return;
                }
            },
        };
        // 移除旧文档，避免重复
        if let Err(e) = self.hybrid.remove(&memory.id).await {
            warn!(target: "indexing", "Remove stale index for {} failed: {}", memory.id, e);
        }
        if let Err(e) = self
            .hybrid
            .add(
                &memory.id,
                &embedding,
                &memory.content,
                Self::payload(memory),
            )
            .await
        {
            warn!(target: "indexing", "Index memory {} failed: {}", memory.id, e);
        }
    }

    /// 从索引移除单条记忆。失败仅告警。
    async fn deindex(&self, id: &Uuid) {
        if let Err(e) = self.hybrid.remove(id).await {
            warn!(target: "indexing", "Deindex memory {} failed: {}", id, e);
        }
    }
}

#[async_trait]
impl MemoryStorage for IndexingStorage {
    async fn save(&self, memory: &Memory) -> Result<()> {
        self.inner.save(memory).await?;
        self.reindex(memory).await;
        Ok(())
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Memory>> {
        self.inner.get(id).await
    }

    async fn update(&self, memory: &Memory) -> Result<()> {
        self.inner.update(memory).await?;
        self.reindex(memory).await;
        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> Result<bool> {
        // 注意：底层 `delete` 的返回值语义是“是否发生硬删”——软删（首次删除）
        // 返回 false、已删记录的再删（硬删）返回 true。因此不能以返回值判断是否
        // 需要摘除索引，这里对所有删除情形一律摘除（对未索引项为无副作用）。
        let result = self.inner.delete(id).await?;
        self.deindex(id).await;
        Ok(result)
    }

    async fn restore(&self, id: &Uuid) -> Result<Option<Memory>> {
        let restored = self.inner.restore(id).await?;
        if let Some(memory) = &restored {
            self.reindex(memory).await;
        }
        Ok(restored)
    }

    async fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        self.inner.list(limit).await
    }

    async fn list_by_type(&self, memory_type: MemoryType, limit: usize) -> Result<Vec<Memory>> {
        self.inner.list_by_type(memory_type, limit).await
    }

    async fn list_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Memory>> {
        self.inner.list_by_tag(tag, limit).await
    }

    async fn list_by_importance(&self, min: f32, max: f32) -> Result<Vec<Memory>> {
        self.inner.list_by_importance(min, max).await
    }

    async fn list_deleted(&self) -> Result<Vec<Memory>> {
        self.inner.list_deleted().await
    }

    async fn count(&self) -> Result<usize> {
        self.inner.count().await
    }

    async fn count_deleted(&self) -> Result<usize> {
        self.inner.count_deleted().await
    }

    async fn list_by_project(&self, project_id: &Uuid) -> Result<Vec<Memory>> {
        self.inner.list_by_project(project_id).await
    }

    async fn list_global(&self, limit: usize) -> Result<Vec<Memory>> {
        self.inner.list_global(limit).await
    }

    async fn list_created_between(
        &self,
        after_unix: i64,
        before_unix: i64,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        self.inner
            .list_created_between(after_unix, before_unix, limit)
            .await
    }

    async fn get_chunks_by_group(&self, chunk_group_id: &Uuid) -> Result<Vec<Memory>> {
        self.inner.get_chunks_by_group(chunk_group_id).await
    }

    async fn list_older_than(&self, cutoff: &str, limit: usize) -> Result<Vec<Memory>> {
        self.inner.list_older_than(cutoff, limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    use crate::embedding::MockEmbedder;
    use crate::storage::traits::{HybridSearchRequest, HybridSearchResult};
    use crate::storage::{MemoryStore, RocksDBStore};

    /// 记录被索引记忆 id 的假混合索引。
    #[derive(Default)]
    struct FakeHybrid {
        ids: Mutex<HashSet<Uuid>>,
        embeddings: Mutex<HashMap<Uuid, Vec<f32>>>,
    }

    impl FakeHybrid {
        fn contains(&self, id: &Uuid) -> bool {
            self.ids.lock().unwrap().contains(id)
        }

        fn embedding_of(&self, id: &Uuid) -> Option<Vec<f32>> {
            self.embeddings.lock().unwrap().get(id).cloned()
        }
    }

    #[async_trait]
    impl HybridStorage for FakeHybrid {
        async fn search(&self, _req: HybridSearchRequest) -> Result<HybridSearchResult> {
            Ok(HybridSearchResult {
                hits: Vec::new(),
                vec_count: 0,
                fts_count: 0,
            })
        }

        async fn add(
            &self,
            id: &Uuid,
            embedding: &[f32],
            _text: &str,
            _payload: VectorPayload,
        ) -> Result<()> {
            self.ids.lock().unwrap().insert(*id);
            self.embeddings
                .lock()
                .unwrap()
                .insert(*id, embedding.to_vec());
            Ok(())
        }

        async fn remove(&self, id: &Uuid) -> Result<bool> {
            Ok(self.ids.lock().unwrap().remove(id))
        }
    }

    fn build() -> (IndexingStorage, Arc<FakeHybrid>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let rocksdb = Arc::new(RocksDBStore::open(dir.path()).unwrap());
        let storage: Arc<dyn MemoryStorage> = Arc::new(MemoryStore::new(rocksdb));
        let hybrid = Arc::new(FakeHybrid::default());
        let embedder: Arc<dyn EmbeddingGenerator> = Arc::new(MockEmbedder::new());
        (
            IndexingStorage::new(storage, hybrid.clone(), embedder),
            hybrid,
            dir,
        )
    }

    #[tokio::test]
    async fn test_save_indexes_memory() {
        let (store, hybrid, _dir) = build();
        let m = Memory::new("index me".to_string(), MemoryType::Knowledge);
        store.save(&m).await.unwrap();
        assert!(hybrid.contains(&m.id), "save 应把记忆写入索引");
    }

    #[tokio::test]
    async fn test_delete_deindexes_memory() {
        let (store, hybrid, _dir) = build();
        let m = Memory::new("delete me".to_string(), MemoryType::Knowledge);
        store.save(&m).await.unwrap();
        assert!(hybrid.contains(&m.id));
        // 软删返回值语义特殊（首次软删为 false），此处只验证索引被摘除
        store.delete(&m.id).await.unwrap();
        assert!(!hybrid.contains(&m.id), "软删后应从索引移除");
    }

    #[tokio::test]
    async fn test_update_reindexes_memory() {
        let (store, hybrid, _dir) = build();
        let mut m = Memory::new("v1".to_string(), MemoryType::Knowledge);
        store.save(&m).await.unwrap();
        m.content = "v2".to_string();
        store.update(&m).await.unwrap();
        assert!(hybrid.contains(&m.id), "更新后应保持索引存在");
    }

    #[tokio::test]
    async fn test_restore_reindexes_memory() {
        let (store, hybrid, _dir) = build();
        let m = Memory::new("restore me".to_string(), MemoryType::Knowledge);
        store.save(&m).await.unwrap();
        store.delete(&m.id).await.unwrap();
        assert!(!hybrid.contains(&m.id));
        assert!(store.restore(&m.id).await.unwrap().is_some());
        assert!(hybrid.contains(&m.id), "恢复后应重建索引");
    }

    #[tokio::test]
    async fn test_reindex_reuses_precomputed_embedding() {
        // 记忆已携带向量时应直接复用，不再调用嵌入生成器（避免 Dream 重复嵌入）
        let (store, hybrid, _dir) = build();
        let mut m = Memory::new("precomputed".to_string(), MemoryType::Knowledge);
        m.embedding = Some(vec![9.0, 8.0, 7.0, 6.0]);
        store.save(&m).await.unwrap();
        assert_eq!(
            hybrid.embedding_of(&m.id).unwrap(),
            vec![9.0, 8.0, 7.0, 6.0],
            "索引应使用记忆预置的向量"
        );
    }
}
