//! 分层引擎单元测试（内联 tests 抽离）。

    use super::*;
    use crate::storage::{GraphSearchHit, GraphTraversalParams};
    use async_trait::async_trait;
    use mr_common::{EdgeType, MemorySource, MemoryType};

    struct MockMemoryStore {
        memories: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<Uuid, Memory>>>,
    }

    impl MockMemoryStore {
        fn new() -> Self {
            MockMemoryStore {
                memories: std::sync::Arc::new(tokio::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
            }
        }

        async fn insert(&self, memory: Memory) {
            let mut map = self.memories.lock().await;
            map.insert(memory.id, memory);
        }
    }

    #[async_trait]
    impl MemoryStorage for MockMemoryStore {
        async fn save(&self, memory: &Memory) -> Result<()> {
            let mut map = self.memories.lock().await;
            map.insert(memory.id, memory.clone());
            Ok(())
        }
        async fn get(&self, id: &Uuid) -> Result<Option<Memory>> {
            let map = self.memories.lock().await;
            Ok(map.get(id).cloned())
        }
        async fn update(&self, memory: &Memory) -> Result<()> {
            self.save(memory).await
        }
        async fn restore(&self, id: &Uuid) -> Result<Option<Memory>> {
            let mut map = self.memories.lock().await;
            match map.get_mut(id) {
                Some(m) if m.is_deleted => {
                    m.is_deleted = false;
                    m.deleted_at = None;
                    Ok(Some(m.clone()))
                }
                _ => Ok(None),
            }
        }
        async fn delete(&self, id: &Uuid) -> Result<bool> {
            let mut map = self.memories.lock().await;
            Ok(map.remove(id).is_some())
        }
        async fn list(&self, limit: usize) -> Result<Vec<Memory>> {
            let map = self.memories.lock().await;
            Ok(map.values().take(limit).cloned().collect())
        }
        async fn list_by_type(
            &self,
            _memory_type: MemoryType,
            limit: usize,
        ) -> Result<Vec<Memory>> {
            self.list(limit).await
        }
        async fn list_by_tag(&self, _tag: &str, limit: usize) -> Result<Vec<Memory>> {
            self.list(limit).await
        }
        async fn list_by_importance(&self, _min: f32, _max: f32) -> Result<Vec<Memory>> {
            self.list(100).await
        }
        async fn list_deleted(&self) -> Result<Vec<Memory>> {
            Ok(vec![])
        }
        async fn count(&self) -> Result<usize> {
            let map = self.memories.lock().await;
            Ok(map.len())
        }
        async fn count_deleted(&self) -> Result<usize> {
            Ok(0)
        }
        async fn list_by_project(&self, _project_id: &Uuid) -> Result<Vec<Memory>> {
            self.list(100).await
        }
        async fn list_global(&self, limit: usize) -> Result<Vec<Memory>> {
            self.list(limit).await
        }
        async fn list_created_between(
            &self,
            _after_unix: i64,
            _before_unix: i64,
            limit: usize,
        ) -> Result<Vec<Memory>> {
            self.list(limit).await
        }
        async fn get_chunks_by_group(&self, _chunk_group_id: &Uuid) -> Result<Vec<Memory>> {
            Ok(vec![])
        }
        async fn list_older_than(&self, _cutoff: &str, limit: usize) -> Result<Vec<Memory>> {
            self.list(limit).await
        }
    }

    struct MockEdgeStore {
        edges: std::sync::Arc<tokio::sync::Mutex<Vec<MemoryEdge>>>,
    }

    impl MockEdgeStore {
        fn new() -> Self {
            MockEdgeStore {
                edges: std::sync::Arc::new(tokio::sync::Mutex::new(vec![])),
            }
        }
    }

    #[async_trait]
    impl EdgeStorage for MockEdgeStore {
        async fn save(&self, edge: &MemoryEdge) -> Result<()> {
            let mut edges = self.edges.lock().await;
            edges.push(edge.clone());
            Ok(())
        }
        async fn get(&self, _id: &Uuid) -> Result<Option<MemoryEdge>> {
            Ok(None)
        }
        async fn delete(&self, _id: &Uuid) -> Result<bool> {
            Ok(false)
        }
        async fn list_out_edges(&self, _source_id: &Uuid) -> Result<Vec<MemoryEdge>> {
            Ok(vec![])
        }
        async fn list_in_edges(&self, _target_id: &Uuid) -> Result<Vec<MemoryEdge>> {
            Ok(vec![])
        }
        async fn list_by_type(&self, _edge_type: EdgeType) -> Result<Vec<MemoryEdge>> {
            Ok(vec![])
        }
        async fn count(&self) -> Result<usize> {
            let edges = self.edges.lock().await;
            Ok(edges.len())
        }
        async fn neighbors(&self, _memory_id: &Uuid) -> Result<Vec<MemoryEdge>> {
            Ok(vec![])
        }
        async fn traverse(
            &self,
            _seed_ids: &[Uuid],
            _params: GraphTraversalParams,
        ) -> Result<Vec<GraphSearchHit>> {
            Ok(vec![])
        }
    }

    struct MockTieredStore {
        summaries: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<Uuid, String>>>,
        keywords: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<Uuid, Vec<String>>>>,
    }

    impl MockTieredStore {
        fn new() -> Self {
            MockTieredStore {
                summaries: std::sync::Arc::new(tokio::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
                keywords: std::sync::Arc::new(tokio::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
            }
        }
    }

    #[async_trait]
    impl TieredStorage for MockTieredStore {
        async fn save_summary(&self, memory_id: &Uuid, summary: &str) -> Result<()> {
            let mut map = self.summaries.lock().await;
            map.insert(*memory_id, summary.to_string());
            Ok(())
        }
        async fn get_summary(&self, memory_id: &Uuid) -> Result<Option<String>> {
            let map = self.summaries.lock().await;
            Ok(map.get(memory_id).cloned())
        }
        async fn delete_summary(&self, memory_id: &Uuid) -> Result<bool> {
            let mut map = self.summaries.lock().await;
            Ok(map.remove(memory_id).is_some())
        }
        async fn save_keywords(&self, memory_id: &Uuid, keywords: &[String]) -> Result<()> {
            let mut map = self.keywords.lock().await;
            map.insert(*memory_id, keywords.to_vec());
            Ok(())
        }
        async fn get_keywords(&self, memory_id: &Uuid) -> Result<Option<Vec<String>>> {
            let map = self.keywords.lock().await;
            Ok(map.get(memory_id).cloned())
        }
        async fn delete_keywords(&self, memory_id: &Uuid) -> Result<bool> {
            let mut map = self.keywords.lock().await;
            Ok(map.remove(memory_id).is_some())
        }
    }

    fn make_memory(content: &str, tags: Vec<&str>) -> Memory {
        use std::collections::HashMap;
        let now = chrono::Utc::now();
        Memory {
            id: Uuid::new_v4(),
            content: content.to_string(),
            memory_type: MemoryType::Decision,
            source: MemorySource::User,
            project_id: None,
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            importance: 0.5,
            created_at: now,
            last_accessed: now,
            access_count: 0,
            summary: None,
            embedding: None,
            metadata: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            chunk_group_id: None,
            chunk_index: None,
            chunk_total: None,
            scope: mr_common::MemoryScope::default(),
        }
    }

    #[tokio::test]
    async fn test_build_tiered_result() {
        let memory_store = MockMemoryStore::new();
        let edge_store = MockEdgeStore::new();
        let tiered_store = MockTieredStore::new();

        let m1 = make_memory("This is a full content test", vec!["test"]);
        let m2 = make_memory("Short content", vec!["short"]);
        let id1 = m1.id;
        let id2 = m2.id;

        memory_store.insert(m1).await;
        memory_store.insert(m2).await;

        let engine = TieredEngine::new(
            memory_store,
            edge_store,
            tiered_store,
            TieredEngineConfig::default(),
        );

        let result = engine
            .build_tiered_result(&[id1, id2], &[0.9, 0.4])
            .await
            .unwrap();

        assert_eq!(result.total_results, 2);
        assert!(result.total_tokens > 0);
        assert_eq!(result.budget, 4000);
        assert_eq!(result.results[0].tier, RepresentationTier::Full);
        assert_eq!(result.results[1].tier, RepresentationTier::Summary);
    }

    #[tokio::test]
    async fn test_truncate_content() {
        let memory_store = MockMemoryStore::new();
        let edge_store = MockEdgeStore::new();
        let tiered_store = MockTieredStore::new();

        let engine = TieredEngine::new(
            memory_store,
            edge_store,
            tiered_store,
            TieredEngineConfig::default(),
        );

        let short = "This is short.";
        let truncated = engine.truncate_content(short, 200);
        assert_eq!(truncated, short);

        let long = "This is a very long content that should be truncated at a sentence boundary. And this is another sentence.";
        let truncated = engine.truncate_content(long, 10);
        assert!(truncated.ends_with("..."));
    }

    #[tokio::test]
    async fn test_tiered_result_with_keywords() {
        let memory_store = MockMemoryStore::new();
        let edge_store = MockEdgeStore::new();
        let tiered_store = MockTieredStore::new();

        let m = make_memory("Content", vec![]);
        let id = m.id;
        memory_store.insert(m).await;

        tiered_store
            .save_keywords(&id, &["rust".to_string(), "async".to_string()])
            .await
            .unwrap();

        let engine = TieredEngine::new(
            memory_store,
            edge_store,
            tiered_store,
            TieredEngineConfig::default(),
        );

        let result = engine.build_tiered_result(&[id], &[0.1]).await.unwrap();

        assert_eq!(result.results[0].tier, RepresentationTier::DenseProxy);
        assert!(result.results[0].content.contains("rust"));
    }
