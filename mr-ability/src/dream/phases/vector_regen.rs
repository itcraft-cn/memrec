//! # Phase 4: 向量重新生成
//!
//! 为缺失向量的记忆重新生成嵌入。

use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tracing::info;

use crate::embedding::EmbeddingGenerator;
use crate::storage::{MemoryStorage, VectorPayload, VectorStorage};

use super::PhaseResult;

pub struct VectorRegenerator {
    storage: Arc<dyn MemoryStorage>,
    vector_store: Arc<dyn VectorStorage>,
    embedder: Arc<dyn EmbeddingGenerator>,
    batch_size: usize,
    batch_interval_ms: u64,
}

impl VectorRegenerator {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        vector_store: Arc<dyn VectorStorage>,
        embedder: Arc<dyn EmbeddingGenerator>,
        batch_size: usize,
        batch_interval_ms: u64,
    ) -> Self {
        Self {
            storage,
            vector_store,
            embedder,
            batch_size,
            batch_interval_ms,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 4 started: VectorRegen");

        let memories = match self.storage.list(10000).await {
            Ok(memories) => memories,
            Err(e) => return PhaseResult::err("VectorRegen", e.to_string()),
        };

        if memories.is_empty() {
            info!(target: "dream", "Dream Phase 4 skipped: no memories");
            return PhaseResult::ok("VectorRegen", 0, 0);
        }

        let mut processed = 0;
        let mut regenerated = 0;

        for batch in memories.chunks(self.batch_size) {
            for memory in batch {
                match self.vector_store.get(&memory.id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        match self.embedder.embed(&memory.content) {
                            Ok(embedding) => {
                                let payload = VectorPayload {
                                    project_id: memory.project_id,
                                    memory_type: memory.memory_type.to_string(),
                                    tags: memory.tags.clone(),
                                    content_preview: memory.content.chars().take(200).collect(),
                                    importance: memory.importance,
                                    chunk_group_id: memory.chunk_group_id,
                                    chunk_index: memory.chunk_index,
                                    chunk_total: memory.chunk_total,
                                };

                                if let Err(e) = self.vector_store.add(&memory.id, &embedding, payload).await {
                                    tracing::warn!("Failed to add vector for {}: {}", memory.id, e);
                                } else {
                                    regenerated += 1;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to embed memory {}: {}", memory.id, e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to check vector for {}: {}", memory.id, e);
                    }
                }
                processed += 1;
            }

            sleep(Duration::from_millis(self.batch_interval_ms)).await;
        }

        info!(
            target: "dream",
            "Dream Phase 4 completed: processed {} memories, regenerated {} vectors",
            processed, regenerated
        );

        PhaseResult::ok("VectorRegen", processed, regenerated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore, VectorStore};
    use mr_common::{Memory, MemoryType};
    use tempfile::tempdir;

    struct MockEmbedder;

    impl EmbeddingGenerator for MockEmbedder {
        fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            Ok(vec![0.1; 384])
        }

        fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.1; 384]).collect())
        }

        fn dimension(&self) -> usize {
            384
        }
    }

    #[tokio::test]
    async fn test_vector_regenerator_empty() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let embedder = Arc::new(MockEmbedder);
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));

        let regenerator = VectorRegenerator::new(
            storage,
            vector_store,
            embedder,
            100,
            10,
        );
        
        let result = regenerator.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 0);
    }

    #[tokio::test]
    async fn test_vector_regenerator_with_memories() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let m1 = Memory::new("test memory".to_string(), MemoryType::Knowledge);
        storage.save(&m1).await.unwrap();

        let embedder = Arc::new(MockEmbedder);
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));

        let regenerator = VectorRegenerator::new(
            storage,
            vector_store,
            embedder,
            100,
            10,
        );
        
        let result = regenerator.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 1);
        assert_eq!(result.created_count, 1);
    }
}
