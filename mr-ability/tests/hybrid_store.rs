//! HybridStore 混合检索集成测试。

mod common;
use common::*;

#[tokio::test]
async fn test_hybrid_store_basic_search() {
    let vector_store = Arc::new(VectorStore::new(3));
    let temp_dir = TempDir::new().unwrap();
    let fts_store = Arc::new(TantivyStore::open(temp_dir.path()).await.unwrap());

    let mmr_config = MmrConfig {
        lambda: 0.5,
        top_k: 10,
        max_candidates: 50,
    };
    let scorer_config = ScorerConfig::default();

    let hybrid = HybridStore::new(
        vector_store.clone(),
        fts_store.clone(),
        mmr_config,
        scorer_config,
    );

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    hybrid
        .add(
            &id1,
            &[1.0, 0.0, 0.0],
            "machine learning algorithms",
            VectorPayload {
                project_id: None,
                memory_type: "knowledge".to_string(),
                tags: vec!["ml".to_string()],
                content_preview: "machine learning algorithms".to_string(),
                importance: 0.8,
                chunk_group_id: None,
                chunk_index: None,
                chunk_total: None,
            },
        )
        .await
        .unwrap();

    hybrid
        .add(
            &id2,
            &[0.0, 1.0, 0.0],
            "deep learning neural networks",
            VectorPayload {
                project_id: None,
                memory_type: "knowledge".to_string(),
                tags: vec!["dl".to_string()],
                content_preview: "deep learning neural networks".to_string(),
                importance: 0.7,
                chunk_group_id: None,
                chunk_index: None,
                chunk_total: None,
            },
        )
        .await
        .unwrap();

    hybrid.reload().unwrap();

    let request = HybridSearchRequest {
        query: "machine learning".to_string(),
        query_embedding: vec![1.0, 0.0, 0.0],
        filter: SearchFilter::default(),
        top_k: 5,
        hybrid_alpha: 0.5,
        mmr_lambda: 0.5,
        mmr_enabled: false,
    };

    let result = hybrid.search(request).await.unwrap();

    assert!(!result.hits.is_empty());
    assert!(result.vec_count > 0 || result.fts_count > 0);
}

#[tokio::test]
async fn test_hybrid_bm25_only() {
    let vector_store = Arc::new(VectorStore::new(3));
    let temp_dir = TempDir::new().unwrap();
    let fts_store = Arc::new(TantivyStore::open(temp_dir.path()).await.unwrap());

    let hybrid = HybridStore::new(
        vector_store,
        fts_store,
        MmrConfig::default(),
        ScorerConfig::default(),
    );

    let id = Uuid::new_v4();
    hybrid
        .add(
            &id,
            &[0.0, 0.0, 0.0],
            "rust programming language",
            VectorPayload {
                content_preview: "rust programming language".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    hybrid.reload().unwrap();

    let request = HybridSearchRequest {
        query: "rust programming".to_string(),
        query_embedding: vec![1.0, 0.0, 0.0],
        filter: SearchFilter::default(),
        top_k: 5,
        hybrid_alpha: 0.0,
        mmr_lambda: 0.5,
        mmr_enabled: false,
    };

    let result = hybrid.search(request).await.unwrap();
    assert!(!result.hits.is_empty());
    assert!(result.fts_count > 0);
}

#[tokio::test]
async fn test_hybrid_vector_only() {
    let vector_store = Arc::new(VectorStore::new(3));
    let temp_dir = TempDir::new().unwrap();
    let fts_store = Arc::new(TantivyStore::open(temp_dir.path()).await.unwrap());

    let hybrid = HybridStore::new(
        vector_store,
        fts_store,
        MmrConfig::default(),
        ScorerConfig::default(),
    );

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    hybrid
        .add(
            &id1,
            &[1.0, 0.0, 0.0],
            "document one",
            VectorPayload {
                content_preview: "document one".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    hybrid
        .add(
            &id2,
            &[0.0, 1.0, 0.0],
            "document two",
            VectorPayload {
                content_preview: "document two".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    hybrid.reload().unwrap();

    let request = HybridSearchRequest {
        query: "unrelated query".to_string(),
        query_embedding: vec![1.0, 0.0, 0.0],
        filter: SearchFilter {
            min_score: 0.0,
            ..Default::default()
        },
        top_k: 5,
        hybrid_alpha: 1.0,
        mmr_lambda: 0.5,
        mmr_enabled: false,
    };

    let result = hybrid.search(request).await.unwrap();
    assert!(!result.hits.is_empty());
    assert!(result.vec_count > 0);
    let hit_ids: Vec<_> = result.hits.iter().map(|h| h.memory_id).collect();
    assert!(hit_ids.contains(&id1));
}

#[tokio::test]
async fn test_hybrid_mmr_enabled() {
    let vector_store = Arc::new(VectorStore::new(3));
    let temp_dir = TempDir::new().unwrap();
    let fts_store = Arc::new(TantivyStore::open(temp_dir.path()).await.unwrap());

    let mmr_config = MmrConfig {
        lambda: 0.5,
        top_k: 10,
        max_candidates: 50,
    };

    let hybrid = HybridStore::new(vector_store, fts_store, mmr_config, ScorerConfig::default());

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();

    hybrid
        .add(
            &id1,
            &[1.0, 0.0, 0.0],
            "alpha beta gamma",
            VectorPayload {
                content_preview: "alpha beta gamma".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    hybrid
        .add(
            &id2,
            &[0.9, 0.1, 0.0],
            "alpha beta delta",
            VectorPayload {
                content_preview: "alpha beta delta".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    hybrid
        .add(
            &id3,
            &[0.5, 0.5, 0.0],
            "completely different text",
            VectorPayload {
                content_preview: "completely different text".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    hybrid.reload().unwrap();

    let request = HybridSearchRequest {
        query: "alpha beta".to_string(),
        query_embedding: vec![1.0, 0.0, 0.0],
        filter: SearchFilter::default(),
        top_k: 3,
        hybrid_alpha: 0.5,
        mmr_lambda: 0.7,
        mmr_enabled: true,
    };

    let result = hybrid.search(request).await.unwrap();
    assert!(!result.hits.is_empty());
}

#[tokio::test]
async fn test_hybrid_remove() {
    let vector_store = Arc::new(VectorStore::new(3));
    let temp_dir = TempDir::new().unwrap();
    let fts_store = Arc::new(TantivyStore::open(temp_dir.path()).await.unwrap());

    let hybrid = HybridStore::new(
        vector_store,
        fts_store,
        MmrConfig::default(),
        ScorerConfig::default(),
    );

    let id = Uuid::new_v4();
    hybrid
        .add(
            &id,
            &[1.0, 0.0, 0.0],
            "test document",
            VectorPayload::default(),
        )
        .await
        .unwrap();

    hybrid.reload().unwrap();

    let removed = hybrid.remove(&id).await.unwrap();
    assert!(removed);

    hybrid.reload().unwrap();

    let request = HybridSearchRequest {
        query: "test document".to_string(),
        query_embedding: vec![1.0, 0.0, 0.0],
        filter: SearchFilter::default(),
        top_k: 5,
        hybrid_alpha: 0.5,
        mmr_lambda: 0.5,
        mmr_enabled: false,
    };

    let result = hybrid.search(request).await.unwrap();
    assert!(result.hits.is_empty() || !result.hits.iter().any(|h| h.memory_id == id));
}


/// T3 验证：MMR 相似度以全文（TextProvider）而非 200 字符 content_preview 计算。
///
/// 构造三条向量分相同（同 embedding）、preview 完全相同的候选 A/B/C：
/// - 全文世界：A、B 全文相同（真重复），C 全文不同 → MMR 第二选应为 C 以保证多样性；
/// - preview 世界（旧行为）：A/B/C 判为完全相似 → 第二选按 id 顺序落到 B。
#[tokio::test]
async fn test_mmr_uses_full_text_via_text_provider() {
    use async_trait::async_trait;
    use mr_ability::storage::TextProvider;
    use std::collections::HashMap;

    // 确定性 id：00000000-...-0001/0002/0003，保证同分排序可预期。
    let id_a = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let id_b = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
    let id_c = Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap();

    // 模拟 provider：A/B 全文相同，C 全文不同（preview 三者相同）。
    struct FakeProvider(HashMap<Uuid, String>);
    #[async_trait]
    impl TextProvider for FakeProvider {
        async fn texts(&self, ids: &[Uuid]) -> anyhow::Result<HashMap<Uuid, String>> {
            Ok(ids
                .iter()
                .filter_map(|id| self.0.get(id).map(|t| (*id, t.clone())))
                .collect())
        }
    }

    let mut fulls = HashMap::new();
    fulls.insert(id_a, "shared full text".to_string());
    fulls.insert(id_b, "shared full text".to_string());
    fulls.insert(id_c, "different conclusion entirely".to_string());

    let temp_dir = TempDir::new().unwrap();
    let fts_store = Arc::new(TantivyStore::open(temp_dir.path()).await.unwrap());
    let vector_store = Arc::new(VectorStore::new(3));
    for id in [&id_a, &id_b, &id_c] {
        vector_store
            .add(
                id,
                &[1.0, 0.0, 0.0],
                VectorPayload {
                    content_preview: "shared preview".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    let request = || HybridSearchRequest {
        query: "irrelevant-no-doc".to_string(),
        query_embedding: vec![1.0, 0.0, 0.0],
        filter: SearchFilter::default(),
        top_k: 3,
        hybrid_alpha: 0.5,
        mmr_lambda: 0.5,
        mmr_enabled: true,
    };

    // 有 provider：A/B 全文相同被判同簇，C 以差异文本胜出第二。
    let hybrid_full = HybridStore::new(
        vector_store.clone(),
        fts_store.clone(),
        MmrConfig::default(),
        ScorerConfig::default(),
    )
    .with_text_provider(Arc::new(FakeProvider(fulls)));
    let hits = hybrid_full.search(request()).await.unwrap().hits;
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0].memory_id, id_a);
    assert_eq!(hits[1].memory_id, id_c, "全文不同应第二选 C");
    assert_eq!(hits[2].memory_id, id_b);

    // 无 provider：三条 preview 完全相同 → 同簇按 id 顺序 A、B、C（旧行为）。
    let hybrid_prev = HybridStore::new(
        vector_store,
        fts_store,
        MmrConfig::default(),
        ScorerConfig::default(),
    );
    let hits = hybrid_prev.search(request()).await.unwrap().hits;
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0].memory_id, id_a);
    assert_eq!(hits[1].memory_id, id_b, "preview 相同应按 id 顺序选 B");
    assert_eq!(hits[2].memory_id, id_c);
}
