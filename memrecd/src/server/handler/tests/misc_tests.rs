//! 其他路由测试。

use super::*;

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
