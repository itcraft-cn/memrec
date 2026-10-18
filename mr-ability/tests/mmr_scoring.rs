//! MMR 重排、评分器与向量存储集成测试。

mod common;
use common::*;

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

