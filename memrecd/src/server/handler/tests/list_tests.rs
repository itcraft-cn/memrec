//! List / Restore 相关测试。

use super::*;

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

