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

mod list_tests;
mod memory_tests;
mod misc_tests;
mod search_tests;
