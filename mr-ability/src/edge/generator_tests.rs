//! 边生成器单元测试（内联 tests 抽离）。

    use super::*;
    use crate::storage::{EdgeStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    async fn create_test_edge_store() -> EdgeStore {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        EdgeStore::new(std::sync::Arc::new(rocksdb))
    }

    fn create_test_memory(content: &str) -> Memory {
        Memory::new(content.to_string(), MemoryType::Knowledge)
    }

    #[tokio::test]
    async fn test_llm_edge_generator_parses_relations() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        let llm = std::sync::Arc::new(MockLlmClient::new(
            r#"[{"target":0,"type":"caused_by","confidence":0.9,"reason":"a caused b"}]"#,
        ));
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);

        let a = create_test_memory("root cause of the crash");
        let b = create_test_memory("crash observed in production");
        let edges = generator
            .generate(&a, &[b.clone()], &edge_store)
            .await
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, EdgeType::CausedBy);
    }

    #[tokio::test]
    async fn test_llm_edge_generator_skips_self_loop() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        // 候选列表同时包含新记忆自身与另一条记忆
        let llm = std::sync::Arc::new(MockLlmClient::new(
            r#"[{"target":0,"type":"references","confidence":0.99,"reason":"same memory"},{"target":1,"type":"evolves","confidence":0.9,"reason":"b evolves a"}]"#,
        ));
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);

        let a = create_test_memory("memory a");
        let b = create_test_memory("memory b");
        let edges = generator
            .generate(&a, &[a.clone(), b.clone()], &edge_store)
            .await
            .unwrap();
        assert_eq!(edges.len(), 1, "自环应被跳过，仅保留指向 b 的边");
        assert_eq!(edges[0].target_id, b.id);
    }

    #[tokio::test]
    async fn test_llm_edge_generator_filters_low_confidence_and_bad_type() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        let llm = std::sync::Arc::new(MockLlmClient::new(
            r#"```json
[{"target":0,"type":"caused_by","confidence":0.2},{"target":1,"type":"unknown","confidence":0.9}]
```"#,
        ));
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);
        let a = create_test_memory("a");
        let edges = generator
            .generate(
                &a,
                &[create_test_memory("b"), create_test_memory("c")],
                &edge_store,
            )
            .await
            .unwrap();
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn test_llm_edge_generator_llm_failure_is_non_fatal() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        let llm = std::sync::Arc::new(MockLlmClient::unconfigured());
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);
        let a = create_test_memory("a");
        let edges = generator
            .generate(&a, &[create_test_memory("b")], &edge_store)
            .await
            .unwrap();
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn test_mock_generator_similar_edges() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::with_defaults();

        let memory_a = create_test_memory("Tokio is a great async runtime for Rust");
        let memory_b = create_test_memory("async-std is another async runtime for Rust");
        let memory_c = create_test_memory("Database optimization techniques");

        let edges = generator
            .generate(&memory_a, &[memory_b.clone(), memory_c], &edge_store)
            .await
            .unwrap();

        assert!(!edges.is_empty());
        assert!(edges[0].edge_type == EdgeType::Similar);
        assert!(edges[0].auto_generated);
    }

    #[tokio::test]
    async fn test_mock_generator_respects_max_edges() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::new(0.3, 2);

        let memory = create_test_memory("Test memory with keywords");
        let candidates: Vec<Memory> = (0..10)
            .map(|i| create_test_memory(&format!("Test memory with keywords {}", i)))
            .collect();

        let edges = generator
            .generate(&memory, &candidates, &edge_store)
            .await
            .unwrap();
        assert!(edges.len() <= 2);
    }

    #[tokio::test]
    async fn test_mock_generator_avoids_self_edge() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::with_defaults();

        let memory = create_test_memory("Test content");
        let edges = generator
            .generate(&memory, std::slice::from_ref(&memory), &edge_store)
            .await
            .unwrap();

        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn test_mock_generator_low_similarity_filtered() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::new(0.8, 10);

        let memory_a = create_test_memory("Rust programming language");
        let memory_b = create_test_memory("Cooking recipes for dinner");

        let edges = generator
            .generate(&memory_a, &[memory_b], &edge_store)
            .await
            .unwrap();

        assert!(edges.is_empty());
    }
