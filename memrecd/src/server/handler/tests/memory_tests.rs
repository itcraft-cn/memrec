//! Add/Delete 相关测试。

use super::*;

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

