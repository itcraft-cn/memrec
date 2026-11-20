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
            max_total_tokens: None,
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
                    max_total_tokens: None,
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
async fn test_search_graph_excludes_soft_deleted_neighbor() {
    // 图扩散仅用于连通，但软删记忆不得作为结果返回。
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

    // B 仅入记忆库（不进索引），随后软删；经 A→B 边扩散到达但应被过滤。
    let b_mem = mr_common::Memory::new(
        "cargo workspace dependency layout".to_string(),
        MemoryType::Knowledge,
    );
    let b_id = b_mem.id;
    router.storage.add(&b_mem).await.unwrap();
    // 新建记忆在恢复窗口内，删除为软删（is_deleted=true，记录仍在库）。
    router.storage.delete(&b_id).await.unwrap();
    let after = router.storage.get(&b_id).await.unwrap();
    assert!(
        after.map(|m| m.is_deleted).unwrap_or(false),
        "B 应为软删状态（记录保留、标记删除）"
    );

    let edge = mr_common::MemoryEdge::new(
        a_id,
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
                max_total_tokens: None,
            })),
            5,
        ))
        .await;
    let results = match resp.result {
        Some(ResponseResult::SemanticSearchResult(r)) => r.results,
        _ => panic!("expected semantic search result"),
    };
    assert!(
        !results.iter().any(|r| r.memory_id == b_id),
        "软删的图邻居不应返回"
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
            max_total_tokens: None,
        })),
        1,
    );

    let response = router.route(request).await;
    assert!(response.result.is_some());
}

#[tokio::test]
async fn test_search_with_token_budget_returns_tiered() {
    // 指定 max_total_tokens 时：返回分层结果，且总 token 不超过预算
    let (router, _edge_store) = router_with_graph();

    let long_content = "背景说明。".repeat(80); // 约 400 汉字，远超 40 token 预算
    router.route(add_request_dedup_off(&long_content, 1)).await;

    let request = JsonRpcRequest::new(
        RequestAction::SearchMemory,
        Some(RequestParams::SearchMemory(SearchMemoryParams {
            query: "背景说明".to_string(),
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
            max_total_tokens: Some(40),
        })),
        2,
    );

    let response = router.route(request).await;
    let result = match response.result {
        Some(ResponseResult::TieredSearchResult(r)) => r,
        other => panic!("期望分层检索结果，实际: {:?}", other.is_some()),
    };

    assert_eq!(result.budget, 40);
    assert!(
        result.total_tokens <= 40,
        "总 token 不得超预算: {}",
        result.total_tokens
    );
    assert!(!result.results.is_empty());
    for hit in &result.results {
        assert!(
            hit.token_count <= 40,
            "单条 token 不得超预算: {}",
            hit.token_count
        );
        assert!(
            !hit.content.is_empty(),
            "分层结果应带渲染后的内容而非仅 preview"
        );
    }
}

#[tokio::test]
async fn test_search_graph_min_ratio_filters_weak_neighbor() {
    // 图命中分是传播分（种子分 × 边权 × weight），量纲不同于相似度。
    // 未设下限时弱邻居会突破用户设定的 min_score；设下限后应被过滤，
    // 而 min_ratio=0 必须恢复旧行为（否则等于悄悄关掉图扩散）。
    async fn neighbor_visible(min_ratio: f32, min_score: f32, edge_weight: f32) -> bool {
        let (router, edge_store) = router_with_graph_opts(0.25, min_ratio);

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

        // B 仅入记忆库、不进索引：只能经图扩散到达。
        let b_mem = mr_common::Memory::new(
            "cargo workspace dependency layout".to_string(),
            MemoryType::Knowledge,
        );
        let b_id = b_mem.id;
        router.storage.add(&b_mem).await.unwrap();

        let edge = mr_common::MemoryEdge::new(
            a_id,
            b_id,
            mr_common::EdgeType::References,
            edge_weight,
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
                    min_score,
                    working_dir: None,
                    hybrid_alpha: default_hybrid_alpha(),
                    mmr_enabled: false,
                    mmr_lambda: default_mmr_lambda(),
                    max_total_tokens: None,
                })),
                5,
            ))
            .await;
        let results = match resp.result {
            Some(ResponseResult::SemanticSearchResult(r)) => r.results,
            _ => panic!("expected semantic search result"),
        };
        results.iter().any(|r| r.memory_id == b_id)
    }

    // 种子分≈1.0、边权 0.5、weight 0.3 → 图命中分≈0.15。
    // min_score=0.9、ratio=0.30 → 下限 0.27，弱邻居应被过滤。
    assert!(
        !neighbor_visible(0.30, 0.9, 0.5).await,
        "弱邻居（0.15 < 0.9×0.30）应被下限过滤"
    );
    // ratio=0 表示不设下限：同一场景必须恢复旧行为。
    assert!(
        neighbor_visible(0.0, 0.9, 0.5).await,
        "min_ratio=0 应保留旧行为，不过滤弱邻居"
    );
    // 下限足够低时弱邻居仍可见，说明机制不是把图扩散整体关掉。
    // 注意 gh.score 已含 graph decay（默认 0.6），故实际图命中分远低于
    // "种子分 × 边权 × weight" 的朴素估算，这里取一个明确极小的比例。
    assert!(
        neighbor_visible(0.01, 0.9, 0.5).await,
        "下限远低于图命中分时应保留邻居"
    );
}
