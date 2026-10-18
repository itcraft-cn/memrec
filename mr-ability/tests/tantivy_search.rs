//! 全文检索（Tantivy）集成测试。

mod common;
use common::*;

#[tokio::test]
async fn test_tantivy_crud() {
    let temp_dir = TempDir::new().unwrap();
    let store = TantivyStore::open(temp_dir.path()).await.unwrap();

    let id = Uuid::new_v4();
    let payload = FtsPayload {
        project_id: None,
        memory_type: "knowledge".to_string(),
        tags: vec!["rust".to_string(), "test".to_string()],
        importance: 0.8,
    };

    store
        .add(&id, "Rust is a systems programming language", payload)
        .await
        .unwrap();
    store.reload().unwrap();

    assert_eq!(store.count().await.unwrap(), 1);

    let filter = SearchFilter::default();
    let results = store.search("Rust programming", filter, 10).await.unwrap();

    assert!(!results.is_empty());
    assert_eq!(results[0].memory_id, id);
    assert!(results[0].score > 0.0);

    let removed = store.remove(&id).await.unwrap();
    assert!(removed);
    store.reload().unwrap();
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn test_tantivy_search_with_filter() {
    let temp_dir = TempDir::new().unwrap();
    let store = TantivyStore::open(temp_dir.path()).await.unwrap();

    let project_id = Uuid::new_v4();

    store
        .add(
            &Uuid::new_v4(),
            "Project specific document",
            FtsPayload {
                project_id: Some(project_id),
                memory_type: "knowledge".to_string(),
                tags: vec![],
                importance: 0.5,
            },
        )
        .await
        .unwrap();

    store
        .add(
            &Uuid::new_v4(),
            "Global document",
            FtsPayload {
                project_id: None,
                memory_type: "knowledge".to_string(),
                tags: vec![],
                importance: 0.5,
            },
        )
        .await
        .unwrap();

    store.reload().unwrap();

    let filter_project = SearchFilter {
        project_id: Some(project_id),
        include_global: false,
        ..Default::default()
    };
    let results = store.search("document", filter_project, 10).await.unwrap();
    assert_eq!(results.len(), 1);

    let filter_with_global = SearchFilter {
        project_id: Some(project_id),
        include_global: true,
        ..Default::default()
    };
    let results = store
        .search("document", filter_with_global, 10)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
}


/// T4 验证：SearchFilter.global_only 只命中全局记忆（project_id None，
/// 历史 Some(nil) 等价处理），向量与全文两侧一致。
#[tokio::test]
async fn test_global_only_filter() {
    let global_id = Uuid::parse_str("00000000-0000-0000-0000-0000000000a0").unwrap();
    let legacy_nil_id = Uuid::parse_str("00000000-0000-0000-0000-0000000000a1").unwrap();
    let project_id = Uuid::parse_str("00000000-0000-0000-0000-0000000000a2").unwrap();
    let project_mem_id = Uuid::parse_str("00000000-0000-0000-0000-0000000000a3").unwrap();

    let temp_dir = TempDir::new().unwrap();
    let fts_store = Arc::new(TantivyStore::open(temp_dir.path()).await.unwrap());
    // 向量侧用 RocksDB 实现（生产语义：按 payload 过滤；内存版 VectorStore 不承载 payload 过滤）
    let temp_vec = TempDir::new().unwrap();
    let vector_store = Arc::new(RocksDBVectorStore::open(temp_vec.path(), 2).unwrap());

    let hybrid = HybridStore::new(
        vector_store.clone(),
        fts_store.clone(),
        MmrConfig::default(),
        ScorerConfig::default(),
    );

    // 全局：None / 历史 nil / 具体项目，各一条；查询词统一命中
    for (id, pid) in [
        (&global_id, None),
        (&legacy_nil_id, Some(Uuid::nil())),
        (&project_mem_id, Some(project_id)),
    ] {
        hybrid
            .add(
                id,
                &[1.0, 0.0],
                "alpha beta searchable",
                VectorPayload {
                    project_id: pid,
                    content_preview: "alpha beta searchable".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    hybrid.reload().unwrap();

    let filter = |global_only: bool| SearchFilter {
        global_only,
        include_global: !global_only,
        ..Default::default()
    };

    // global_only：仅 None + 历史 nil 两条（向量侧）
    let vec_hits = vector_store
        .search(&[1.0, 0.0], filter(true), 10)
        .await
        .unwrap();
    assert_eq!(vec_hits.len(), 2);
    assert!(!vec_hits.iter().any(|h| h.memory_id == project_mem_id));

    // 全文侧：global_only 同样只留全局（不因 include_global 放行所有文档）
    let fts_hits = fts_store
        .search("searchable", filter(true), 10)
        .await
        .unwrap();
    assert_eq!(fts_hits.len(), 2);
    assert!(!fts_hits.iter().any(|h| h.memory_id == project_mem_id));

    // 混合层 global_only：只返回全局命中
    let hybrid_hits = hybrid
        .search(HybridSearchRequest {
            query: "searchable".to_string(),
            query_embedding: vec![1.0, 0.0],
            filter: filter(true),
            top_k: 10,
            hybrid_alpha: 0.5,
            mmr_lambda: 0.5,
            mmr_enabled: false,
        })
        .await
        .unwrap()
        .hits;
    assert_eq!(hybrid_hits.len(), 2);
    assert!(!hybrid_hits.iter().any(|h| h.memory_id == project_mem_id));
}
