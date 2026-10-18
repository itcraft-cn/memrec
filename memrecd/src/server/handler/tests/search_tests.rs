//! 检索与图扩散相关测试。

use super::*;

    #[tokio::test]
    async fn test_search_graph_diffusion_brings_neighbor() {
        // P1-6：seed 命中经图扩散补充召回，邻居带 EdgeHint 标注返回
        let (router, edge_store) = router_with_graph();

        let a = router
            .route(add_request_dedup_off(
                "rust ownership and borrowing rules",
                1,
            ))
            .await;
        let a_id = match &a.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory"),
        };

        // B 只写入记忆库、不进入索引，保证只能经图扩散到达（排除 seed 干扰）
        let b_mem = mr_common::Memory::new(
            "cargo workspace dependency layout".to_string(),
            MemoryType::Knowledge,
        );
        let b_id = b_mem.id;
        router.storage.add(&b_mem).await.unwrap();

        // A → B 建 References 边
        let edge = mr_common::MemoryEdge::new(
            a_id,
            b_id,
            mr_common::EdgeType::References,
            0.9,
            "test".to_string(),
        );
        edge_store.save(&edge).await.unwrap();

        // 仅查询 A 的内容
        let req = JsonRpcRequest::new(
            RequestAction::SearchMemory,
            Some(RequestParams::SearchMemory(SearchMemoryParams {
                query: "rust ownership and borrowing rules".to_string(),
                project_id: None,
                include_global: true,
                project_only: false,
                global_only: false,
                cross_project: false,
                memory_type: None,
                top_k: 5,
                min_score: 0.0,
                working_dir: None,
                hybrid_alpha: default_hybrid_alpha(),
                mmr_enabled: false,
                mmr_lambda: default_mmr_lambda(),
            })),
            3,
        );
        let resp = router.route(req).await;
        let results = match resp.result {
            Some(ResponseResult::SemanticSearchResult(r)) => r.results,
            _ => panic!("expected semantic search result"),
        };

        let a_hit = results
            .iter()
            .find(|r| r.memory_id == a_id)
            .expect("A 应命中");
        assert!(a_hit.edge_hint.is_none(), "seed 命中不应带 EdgeHint");
        let b_hit = results
            .iter()
            .find(|r| r.memory_id == b_id)
            .expect("邻居 B 应经图扩散进入结果");
        assert!(b_hit.edge_hint.is_some(), "图命中应带 EdgeHint");
    }

    #[tokio::test]
    async fn test_search_graph_quota_reserves_slot() {
        // 种子填满 top_k 时：quota=0 图命中被截断；quota>0 图命中获保留名额。
        // 构造 helper：返回 (图命中是否进入结果)。
        async fn has_graph_hit(quota: f32) -> bool {
            let (router, edge_store) = router_with_graph_quota(quota);

            // 5 条同内容种子，恰好填满 top_k=5
            let mut seed_ids = Vec::new();
            for i in 0..5u64 {
                let r = router
                    .route(add_request_dedup_off(
                        "rust ownership and borrowing rules",
                        100 + i,
                    ))
                    .await;
                if let Some(ResponseResult::Memory(m)) = r.result {
                    seed_ids.push(m.memory.id);
                }
            }
            assert_eq!(seed_ids.len(), 5);

            // B 仅写入记忆库、不进索引，只能经图扩散到达
            let b_mem = mr_common::Memory::new(
                "cargo workspace dependency layout".to_string(),
                MemoryType::Knowledge,
            );
            let b_id = b_mem.id;
            router.storage.add(&b_mem).await.unwrap();
            let edge = mr_common::MemoryEdge::new(
                seed_ids[0],
                b_id,
                mr_common::EdgeType::References,
                0.9,
                "test".to_string(),
            );
            edge_store.save(&edge).await.unwrap();

            let resp = router
                .route(JsonRpcRequest::new(
                    RequestAction::SearchMemory,
                    Some(RequestParams::SearchMemory(SearchMemoryParams {
                        query: "rust ownership and borrowing rules".to_string(),
                        project_id: None,
                        include_global: true,
                        project_only: false,
                        global_only: false,
                        cross_project: false,
                        memory_type: None,
                        top_k: 5,
                        min_score: 0.0,
                        working_dir: None,
                        hybrid_alpha: default_hybrid_alpha(),
                        mmr_enabled: false,
                        mmr_lambda: default_mmr_lambda(),
                    })),
                    7,
                ))
                .await;
            match resp.result {
                Some(ResponseResult::SemanticSearchResult(r)) => {
                    r.results.iter().any(|x| x.memory_id == b_id)
                }
                _ => panic!("expected semantic search result"),
            }
        }

        assert!(
            !has_graph_hit(0.0).await,
            "quota=0 时图命中应按分截断（图命中分恒低于种子分）"
        );
        assert!(
            has_graph_hit(0.25).await,
            "quota>0 时图命中应获得保留名额并进入结果"
        );
    }

    #[tokio::test]
    async fn test_router_search_memory() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));
        let embedder = Arc::new(MockEmbedder::new());
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(TantivyStore::new_test());
        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));

        let dream_config = DreamConfig::default();
        let data_dir = std::path::PathBuf::from("/tmp/memrec_test");
        let dream_processor = Arc::new(DreamProcessor::new(
            dream_config,
            &data_dir,
            storage.clone(),
        ));
        let archive_manager = ArchiveManager::new(data_dir.clone(), true);

        let rocksdb_for_scene = Arc::new(RocksDBStore::open(&data_dir).unwrap());
        let scene_store = Arc::new(SceneStore::new(rocksdb_for_scene));

        let router = Router::new(
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
            DedupConfig::default(),
            None,
        );

        let request = JsonRpcRequest::new(
            RequestAction::SearchMemory,
            Some(RequestParams::SearchMemory(SearchMemoryParams {
                query: "test query".to_string(),
                project_id: None,
                include_global: true,
                project_only: false,
                global_only: false,
                cross_project: false,
                memory_type: None,
                top_k: 10,
                min_score: 0.0,
                working_dir: None,
                hybrid_alpha: default_hybrid_alpha(),
                mmr_enabled: default_mmr_enabled(),
                mmr_lambda: default_mmr_lambda(),
            })),
            1,
        );

        let response = router.route(request).await;
        assert!(response.result.is_some());
    }

