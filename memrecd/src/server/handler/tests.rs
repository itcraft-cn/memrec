//! Router 单元测试（原 handler.rs 内联 tests 模块抽离）。

    use super::*;
    use mr_ability::dedup::RuleDedupDecider;
    use mr_ability::embedding::MockEmbedder;
    use mr_ability::search::{MmrConfig, ScorerConfig};
    use mr_ability::storage::rocksdb::RocksDBStore;
    use mr_ability::storage::{
        EdgeStore, HybridStore, MemoryStore, SceneStore, TantivyStore, VectorStore,
    };
    use mr_common::DreamConfig;
    use mr_protocol::{
        default_hybrid_alpha, default_mmr_enabled, default_mmr_lambda, AddParams,
        GetProjectInfoParams, ListParams, SearchMemoryParams,
    };
    use tempfile::tempdir;

    /// 可预测相似度的测试嵌入器。
    ///
    /// 每个字节映射到独立维度，相同内容得到相同向量（相似度 1.0），
    /// 共享前缀越长相似度越高，便于控制去重测试场景。
    struct TestEmbedder;

    impl mr_ability::embedding::EmbeddingGenerator for TestEmbedder {
        fn dimension(&self) -> usize {
            384
        }

        fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            let mut v = vec![0.0f32; 384];
            for (i, b) in text.bytes().take(384).enumerate() {
                v[i] = b as f32 / 255.0;
            }
            Ok(v)
        }

        fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|t| self.embed(t).unwrap()).collect())
        }
    }

    /// 构造带去重配置的路由器（注入 TestEmbedder + 规则决策器）。
    fn router_with_dedup(dup_threshold: f32, update_threshold: f32) -> Router {
        router_with_dedup_full(dup_threshold, update_threshold, 0)
    }

    /// 构建带去重配置的 Router；`update_window_mins` 控制原地更新窗口
    /// （0 表示不限制，便于既有用例）。
    fn router_with_dedup_full(
        dup_threshold: f32,
        update_threshold: f32,
        update_window_mins: u32,
    ) -> Router {
        let dir = tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();
        // 保持临时目录存活（Router 生命周期内 RocksDB 需要访问数据文件）
        std::mem::forget(dir);
        // 共享 RocksDB 实例（MemoryStore 与 SceneStore 复用，避免锁冲突）
        let rocksdb = Arc::new(RocksDBStore::open(&data_dir).unwrap());
        let storage = Arc::new(MemoryStore::new(rocksdb.clone()));
        let embedder = Arc::new(TestEmbedder);
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(TantivyStore::new_test());
        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));

        let dream_config = DreamConfig::default();
        let dream_processor = Arc::new(DreamProcessor::new(
            dream_config,
            &data_dir,
            storage.clone(),
        ));
        let archive_manager = ArchiveManager::new(data_dir.clone(), true);
        let scene_store = Arc::new(SceneStore::new(rocksdb));

        let dedup_config = DedupConfig {
            enabled: true,
            top_k: 5,
            duplicate_threshold: dup_threshold,
            update_threshold,
            update_window_mins,
        };
        let decider: Option<Arc<dyn DedupDecider>> =
            Some(Arc::new(RuleDedupDecider::from_config(&dedup_config)));

        Router::new(
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
            dedup_config,
            decider,
        )
    }

    /// 构建注入了边存储的 Router，用于检索图扩散测试。
    ///
    /// 返回 `(Router, EdgeStore)`：测试可手动建边后验证扩散召回。
    fn router_with_graph() -> (Router, Arc<dyn mr_ability::storage::EdgeStorage>) {
        router_with_graph_quota(0.25)
    }

    /// 同上，但显式指定图命中保留名额比例，用于验证保留名额机制。
    fn router_with_graph_quota(quota: f32) -> (Router, Arc<dyn mr_ability::storage::EdgeStorage>) {
        let dir = tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();
        std::mem::forget(dir);
        let rocksdb = Arc::new(RocksDBStore::open(&data_dir).unwrap());
        let storage = Arc::new(MemoryStore::new(rocksdb.clone()));
        let embedder = Arc::new(TestEmbedder);
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(TantivyStore::new_test());
        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));

        let dream_config = DreamConfig::default();
        let dream_processor = Arc::new(DreamProcessor::new(
            dream_config,
            &data_dir,
            storage.clone(),
        ));
        let archive_manager = ArchiveManager::new(data_dir.clone(), true);
        let edge_store: Arc<dyn mr_ability::storage::EdgeStorage> =
            Arc::new(EdgeStore::new(rocksdb.clone()));
        let scene_store = Arc::new(SceneStore::new(rocksdb));

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
        )
        .with_edge_store(edge_store.clone())
        .with_retrieval_graph(RetrievalGraph {
            enabled: true,
            depth: 1,
            decay: 0.6,
            weight: 0.3,
            quota,
        });
        (router, edge_store)
    }

    fn add_request(content: &str, id: u64) -> JsonRpcRequest {
        JsonRpcRequest::new(
            RequestAction::Add,
            Some(RequestParams::Add(AddParams {
                content: content.to_string(),
                memory_type: MemoryType::Knowledge,
                tags: vec![],
                project_id: None,
                is_global: true,
                working_dir: None,
                source: None,
                scope: None,
                dedup: None,
            })),
            id,
        )
    }

    fn add_request_dedup_off(content: &str, id: u64) -> JsonRpcRequest {
        let mut req = add_request(content, id);
        if let Some(RequestParams::Add(p)) = &mut req.params {
            p.dedup = Some(false);
        }
        req
    }

    fn extract_action(response: &JsonRpcResponse) -> String {
        match &response.result {
            Some(ResponseResult::Memory(m)) => m.action.clone(),
            _ => String::new(),
        }
    }

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
    async fn test_add_duplicate_skipped() {
        let router = router_with_dedup(0.95, 0.80);
        let content = "jwt auth uses RS256 signing";

        let first = router.route(add_request(content, 1)).await;
        assert_eq!(extract_action(&first), "added");

        let second = router.route(add_request(content, 2)).await;
        assert_eq!(extract_action(&second), "skipped");

        // 记忆库只有一条
        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_add_update_merges_existing() {
        let router = router_with_dedup(0.95, 0.80);
        let base = "jwt auth uses RS256 signing";
        let evolved = "jwt auth uses RS256 signing rotation";

        let first = router.route(add_request(base, 1)).await;
        let first_id = match &first.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };

        let second = router.route(add_request(evolved, 2)).await;
        assert_eq!(extract_action(&second), "updated");
        let updated = match &second.result {
            Some(ResponseResult::Memory(m)) => m.memory.clone(),
            _ => panic!("expected memory result"),
        };
        // 更新保留了原 ID，内容为新内容
        assert_eq!(updated.id, first_id);
        assert!(updated.content.contains("rotation"));
        let duplicates = match &second.result {
            Some(ResponseResult::Memory(m)) => m.duplicates.clone(),
            _ => panic!("expected memory result"),
        };
        assert!(!duplicates.is_empty());

        // 记忆库仍只有一条
        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_add_update_window_outside_marks_superseded() {
        // 附则 A.1：超出更新窗口的"修正"不覆盖历史，改为新增 + 旧记录标记 superseded
        let router = router_with_dedup_full(0.95, 0.80, 1);
        let base = "jwt auth uses RS256 signing";
        let evolved = "jwt auth uses RS256 signing rotation";

        let first = router.route(add_request(base, 1)).await;
        let first_id = match &first.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };

        // 将旧记忆老化到窗口之外（窗口 1 分钟）
        let mut old = router.storage.get(&first_id).await.unwrap().unwrap();
        old.created_at = chrono::Utc::now() - chrono::Duration::minutes(120);
        router.storage.update(&old).await.unwrap();

        let second = router.route(add_request(evolved, 2)).await;
        assert_eq!(extract_action(&second), "added");
        let second_id = match &second.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };

        // 旧记录保留且被标记 superseded
        let old_after = router.storage.get(&first_id).await.unwrap().unwrap();
        assert!(old_after.tags.iter().any(|t| t == "superseded"));
        assert_eq!(
            old_after.metadata.get("superseded_by").map(|s| s.as_str()),
            Some(second_id.to_string().as_str())
        );

        // 两条记录并存（未覆盖、未删除）
        assert_eq!(router.storage.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_add_unrelated_adds() {
        let router = router_with_dedup(0.95, 0.80);

        let first = router.route(add_request("jwt auth uses RS256", 1)).await;
        assert_eq!(extract_action(&first), "added");

        let second = router
            .route(add_request("rust ownership and borrowing", 2))
            .await;
        assert_eq!(extract_action(&second), "added");

        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_add_dedup_explicitly_disabled() {
        let router = router_with_dedup(0.95, 0.80);
        let content = "jwt auth uses RS256 signing";

        let first = router.route(add_request(content, 1)).await;
        assert_eq!(extract_action(&first), "added");

        // dedup=false 显式关闭，重复内容也写入
        let second = router.route(add_request_dedup_off(content, 2)).await;
        assert_eq!(extract_action(&second), "added");

        let count = router.storage.count().await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_delete_removes_search_index() {
        let router = router_with_dedup(0.95, 0.80);
        let content = "delete index test unique token abc123";

        let add = router.route(add_request(content, 1)).await;
        let mem_id = match &add.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };

        // 删除前可检索到
        let before = router
            .vector_store
            .search(
                &router.embedder.embed(content).unwrap(),
                SearchFilter {
                    project_id: None,
                    include_global: true,
                    memory_type: None,
                    min_score: 0.5,
                    ..Default::default()
                },
                5,
            )
            .await
            .unwrap();
        assert!(!before.is_empty(), "delete 前应能检索到");

        let del = router
            .route(JsonRpcRequest::new(
                RequestAction::Delete,
                Some(RequestParams::Delete(mr_protocol::DeleteParams {
                    id: mem_id,
                    force: false,
                })),
                2,
            ))
            .await;
        assert!(del.result.is_some());

        // 删除后索引同步移除，不再命中
        let after = router
            .vector_store
            .search(
                &router.embedder.embed(content).unwrap(),
                SearchFilter {
                    project_id: None,
                    include_global: true,
                    memory_type: None,
                    min_score: 0.5,
                    ..Default::default()
                },
                5,
            )
            .await
            .unwrap();
        assert!(
            after.iter().all(|h| h.memory_id != mem_id),
            "delete 后索引应移除该记忆"
        );
    }

    #[tokio::test]
    async fn test_list_filters_tags_and_type() {
        let router = router_with_dedup(0.95, 0.80);

        router
            .route(add_request("rust trait object pattern", 1))
            .await;
        let add2 = router.route(add_request("java stream pipeline", 2)).await;
        // 给第二条打上 java 标签
        let mem_id2 = match &add2.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };
        if let Ok(Some(mut m)) = router.storage.get(&mem_id2).await {
            m.tags = vec!["java".to_string(), "lang".to_string()];
            m.memory_type = mr_common::MemoryType::Knowledge;
            router.storage.update(&m).await.unwrap();
        }

        // 按标签过滤
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: vec!["java".to_string()],
                    memory_type: None,
                    limit: 20,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: false,
                    skip: 0,
                })),
                3,
            ))
            .await;
        let list = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert_eq!(list.memories.len(), 1);
        assert_eq!(list.memories[0].id, mem_id2);

        // 按类型过滤（第二条为 knowledge）
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: Vec::new(),
                    memory_type: Some(mr_common::MemoryType::Knowledge),
                    limit: 20,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: false,
                    skip: 0,
                })),
                4,
            ))
            .await;
        let list = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert_eq!(list.memories.len(), 1);
        assert_eq!(list.memories[0].id, mem_id2);
    }

    #[tokio::test]
    async fn test_list_include_deleted() {
        let router = router_with_dedup(0.95, 0.80);

        let add = router
            .route(add_request("to be deleted raw record", 1))
            .await;
        let mem_id = match &add.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };

        // 软删除
        router
            .route(JsonRpcRequest::new(
                RequestAction::Delete,
                Some(RequestParams::Delete(mr_protocol::DeleteParams {
                    id: mem_id,
                    force: false,
                })),
                2,
            ))
            .await;

        // 默认列表不含已删除
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: Vec::new(),
                    memory_type: None,
                    limit: 20,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: false,
                    skip: 0,
                })),
                3,
            ))
            .await;
        let list = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert!(list.memories.iter().all(|m| m.id != mem_id));

        // include_deleted 应包含软删除的原始记录
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: Vec::new(),
                    memory_type: None,
                    limit: 20,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: true,
                    skip: 0,
                })),
                4,
            ))
            .await;
        let list = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert!(
            list.memories.iter().any(|m| m.id == mem_id && m.is_deleted),
            "include_deleted 应返回软删除记录"
        );
    }

    #[tokio::test]
    async fn test_restore_recovers_and_reindexes() {
        let router = router_with_dedup(0.95, 0.80);
        let content = "restore me unique token xyz987";

        let add = router.route(add_request(content, 1)).await;
        let mem_id = match &add.result {
            Some(ResponseResult::Memory(m)) => m.memory.id,
            _ => panic!("expected memory result"),
        };

        // 软删除（索引同步移除）
        router
            .route(JsonRpcRequest::new(
                RequestAction::Delete,
                Some(RequestParams::Delete(mr_protocol::DeleteParams {
                    id: mem_id,
                    force: false,
                })),
                2,
            ))
            .await;

        // 恢复
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::Restore,
                Some(RequestParams::Restore(mr_protocol::RestoreParams {
                    id: mem_id,
                })),
                3,
            ))
            .await;
        let restored = match resp.result {
            Some(ResponseResult::Memory(m)) => m.memory,
            _ => panic!("expected memory result"),
        };
        assert!(!restored.is_deleted, "恢复后 is_deleted 应为 false");
        assert!(restored.deleted_at.is_none());

        // 恢复后重新入索引，可再次检索命中
        let hits = router
            .vector_store
            .search(
                &router.embedder.embed(content).unwrap(),
                SearchFilter {
                    project_id: None,
                    include_global: true,
                    memory_type: None,
                    min_score: 0.5,
                    ..Default::default()
                },
                5,
            )
            .await
            .unwrap();
        assert!(
            hits.iter().any(|h| h.memory_id == mem_id),
            "restore 后应重新入检索索引"
        );

        // 重复恢复应报错
        let resp2 = router
            .route(JsonRpcRequest::new(
                RequestAction::Restore,
                Some(RequestParams::Restore(mr_protocol::RestoreParams {
                    id: mem_id,
                })),
                4,
            ))
            .await;
        assert!(resp2.error.is_some(), "非删除状态重复恢复应报错");
    }

    #[tokio::test]
    async fn test_list_pagination_skip() {
        let router = router_with_dedup(0.95, 0.80);

        // 添加 5 条记忆
        let mut ids = Vec::new();
        for i in 0..5 {
            let add = router
                .route(add_request_dedup_off(
                    &format!("pagination item {}", i),
                    i as u64 + 1,
                ))
                .await;
            match add.result {
                Some(ResponseResult::Memory(m)) => ids.push(m.memory.id),
                _ => panic!("expected memory result"),
            }
        }

        // 第一页：limit=2, skip=0 → 前 2 条
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: Vec::new(),
                    memory_type: None,
                    limit: 2,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: false,
                    skip: 0,
                })),
                99,
            ))
            .await;
        let list = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert_eq!(list.memories.len(), 2);
        // total 为符合条件的总数（5 条），用于判断是否还有下一页
        assert_eq!(list.total, 5);

        // 第二页：skip=2 → 第 3、4 条，与第一页不重叠
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: Vec::new(),
                    memory_type: None,
                    limit: 2,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: false,
                    skip: 2,
                })),
                100,
            ))
            .await;
        let list2 = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert_eq!(list2.memories.len(), 2);
        let page1: Vec<_> = list.memories.iter().map(|m| m.id).collect();
        let page2: Vec<_> = list2.memories.iter().map(|m| m.id).collect();
        assert!(
            page1.iter().all(|id| !page2.contains(id)),
            "第二页不应与第一页重叠"
        );

        // 第三页：skip=4 → 最后 1 条
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: Vec::new(),
                    memory_type: None,
                    limit: 2,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: false,
                    skip: 4,
                })),
                101,
            ))
            .await;
        let list3 = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert_eq!(list3.memories.len(), 1);
        assert_eq!(list3.total, 5);

        // 超出范围：skip 超过总数 → 空页，total 仍准确
        let resp = router
            .route(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: Vec::new(),
                    memory_type: None,
                    limit: 2,
                    project_only: false,
                    global_only: false,
                    project_id: None,
                    include_deleted: false,
                    skip: 100,
                })),
                102,
            ))
            .await;
        let list4 = match resp.result {
            Some(ResponseResult::MemoryList(l)) => l,
            _ => panic!("expected memory list"),
        };
        assert!(list4.memories.is_empty());
        assert_eq!(list4.total, 5);
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

    #[tokio::test]
    async fn test_router_project_info() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let router = Router::new_simple(storage);

        let request = JsonRpcRequest::new(
            RequestAction::GetProjectInfo,
            Some(RequestParams::GetProjectInfo(
                GetProjectInfoParams::default(),
            )),
            1,
        );

        let response = router.route(request).await;
        assert!(response.result.is_some() || response.error.is_some());
    }
