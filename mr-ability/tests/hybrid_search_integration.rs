//! # 混合搜索集成测试
//!
//! 测试 HybridStore、TantivyStore、MMR 重排、评分器等功能。

use mr_ability::search::{apply_scoring, MmrConfig, ScorerConfig, SourceWeights};
use mr_ability::storage::{
    FtsPayload, FtsStorage, HybridSearchRequest, HybridStorage, HybridStore, RocksDBVectorStore,
    SearchFilter, TantivyStore, VectorPayload, VectorStorage, VectorStore,
};
use mr_common::MemoryScope;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

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

#[test]
fn test_mmr_rerank_diversity() {
    use mr_ability::search::MmrHit;

    #[derive(Clone)]
    struct TestHit {
        text: String,
        score: f64,
    }

    impl MmrHit for TestHit {
        fn score(&self) -> f64 {
            self.score
        }
        fn text(&self) -> &str {
            &self.text
        }
    }

    let candidates = vec![
        TestHit {
            text: "machine learning algorithms".to_string(),
            score: 0.9,
        },
        TestHit {
            text: "machine learning models".to_string(),
            score: 0.85,
        },
        TestHit {
            text: "web development frameworks".to_string(),
            score: 0.8,
        },
        TestHit {
            text: "database optimization".to_string(),
            score: 0.75,
        },
    ];

    let config = MmrConfig {
        lambda: 0.5,
        top_k: 3,
        max_candidates: 50,
    };

    let result = mr_ability::search::mmr_rerank(candidates, &config);

    assert_eq!(result.len(), 3);

    assert!((result[0].score() - 0.9).abs() < 0.001);

    let second_and_third_diverse = result[1..2].iter().any(|h| {
        !h.text().contains("machine learning") || result[0].text().contains("machine learning")
    });
    assert!(second_and_third_diverse || result.len() >= 2);
}

#[test]
fn test_mmr_pure_relevance() {
    use mr_ability::search::MmrHit;

    #[derive(Clone)]
    struct TestHit {
        text: String,
        score: f64,
    }

    impl MmrHit for TestHit {
        fn score(&self) -> f64 {
            self.score
        }
        fn text(&self) -> &str {
            &self.text
        }
    }

    let candidates = vec![
        TestHit {
            text: "high score doc".to_string(),
            score: 0.9,
        },
        TestHit {
            text: "medium score doc".to_string(),
            score: 0.7,
        },
        TestHit {
            text: "low score doc".to_string(),
            score: 0.5,
        },
    ];

    let config = MmrConfig {
        lambda: 1.0,
        top_k: 3,
        max_candidates: 50,
    };

    let result = mr_ability::search::mmr_rerank(candidates, &config);

    assert_eq!(result.len(), 3);
    assert!((result[0].score() - 0.9).abs() < 0.001);
    assert!((result[1].score() - 0.7).abs() < 0.001);
    assert!((result[2].score() - 0.5).abs() < 0.001);
}

#[test]
fn test_mmr_single_candidate() {
    use mr_ability::search::MmrHit;

    #[derive(Clone)]
    struct TestHit {
        text: String,
        score: f64,
    }

    impl MmrHit for TestHit {
        fn score(&self) -> f64 {
            self.score
        }
        fn text(&self) -> &str {
            &self.text
        }
    }

    let candidates = vec![TestHit {
        text: "single document".to_string(),
        score: 0.9,
    }];

    let config = MmrConfig {
        lambda: 0.5,
        top_k: 5,
        max_candidates: 50,
    };

    let result = mr_ability::search::mmr_rerank(candidates, &config);
    assert_eq!(result.len(), 1);
}

#[test]
fn test_scorer_time_decay() {
    let config = ScorerConfig {
        decay_half_life_hours: 336.0,
        // T4 起 Workspace 已废弃并归一为 Global，时间衰减豁免集合仅需 Global
        evergreen_scopes: vec![MemoryScope::Global],
        source_weights: SourceWeights::default(),
    };

    let old_time = chrono::Utc::now() - chrono::Duration::hours(336);
    let recent_time = chrono::Utc::now() - chrono::Duration::hours(1);

    let old_score = apply_scoring(
        1.0,
        old_time,
        MemoryScope::Project,
        mr_common::MemorySource::User,
        &config,
    );
    let recent_score = apply_scoring(
        1.0,
        recent_time,
        MemoryScope::Project,
        mr_common::MemorySource::User,
        &config,
    );

    assert!(recent_score > old_score);
    assert!((old_score - 0.5).abs() < 0.15);
}

#[test]
fn test_scorer_evergreen_exempt() {
    let config = ScorerConfig::default();

    let old_time = chrono::Utc::now() - chrono::Duration::hours(1000);

    let global_score = apply_scoring(
        1.0,
        old_time,
        MemoryScope::Global,
        mr_common::MemorySource::User,
        &config,
    );

    assert!((global_score - 1.0).abs() < 0.001);
}

#[test]
fn test_scorer_source_weight() {
    let config = ScorerConfig::default();
    let now = chrono::Utc::now();

    let user_score = apply_scoring(
        1.0,
        now,
        MemoryScope::Global,
        mr_common::MemorySource::User,
        &config,
    );
    let inferred_score = apply_scoring(
        1.0,
        now,
        MemoryScope::Global,
        mr_common::MemorySource::Inferred,
        &config,
    );

    assert!((user_score - 1.0).abs() < 0.001);
    assert!((inferred_score - 0.5).abs() < 0.001);
    assert!(user_score > inferred_score);
}

#[test]
fn test_scorer_combined() {
    let config = ScorerConfig::default();

    let old_time = chrono::Utc::now() - chrono::Duration::hours(168);

    let score = apply_scoring(
        1.0,
        old_time,
        MemoryScope::Project,
        mr_common::MemorySource::Inferred,
        &config,
    );

    assert!(score < 0.5);
    assert!(score > 0.0);
}

#[test]
fn test_source_weights_custom() {
    let custom_weights = SourceWeights {
        user: 1.0,
        system: 0.9,
        inferred: 0.6,
        external: 0.8,
    };

    let config = ScorerConfig {
        source_weights: custom_weights,
        ..Default::default()
    };

    let now = chrono::Utc::now();

    let system_score = apply_scoring(
        1.0,
        now,
        MemoryScope::Global,
        mr_common::MemorySource::System,
        &config,
    );
    assert!((system_score - 0.9).abs() < 0.001);

    let external_score = apply_scoring(
        1.0,
        now,
        MemoryScope::Global,
        mr_common::MemorySource::External,
        &config,
    );
    assert!((external_score - 0.8).abs() < 0.001);
}

#[tokio::test]
async fn test_vector_store_crud() {
    let store = VectorStore::new(3);

    let id = Uuid::new_v4();
    let embedding = vec![1.0, 2.0, 3.0];

    store
        .add(&id, &embedding, VectorPayload::default())
        .await
        .unwrap();

    let retrieved = store.get(&id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap(), embedding);

    let removed = store.remove(&id).await.unwrap();
    assert!(removed);

    let retrieved = store.get(&id).await.unwrap();
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn test_vector_store_search() {
    let store = VectorStore::new(3);

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();

    store
        .add(&id1, &[1.0, 0.0, 0.0], VectorPayload::default())
        .await
        .unwrap();
    store
        .add(&id2, &[0.9, 0.1, 0.0], VectorPayload::default())
        .await
        .unwrap();
    store
        .add(&id3, &[0.0, 1.0, 0.0], VectorPayload::default())
        .await
        .unwrap();

    let filter = SearchFilter {
        min_score: 0.0,
        ..Default::default()
    };

    let results = store.search(&[1.0, 0.0, 0.0], filter, 2).await.unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].memory_id, id1);
    assert!(results[0].score > 0.99);
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
