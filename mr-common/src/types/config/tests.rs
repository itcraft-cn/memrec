//! 配置默认值单元测试（内联 tests 抽离）。

use super::*;

    #[test]
    fn test_memory_config_defaults() {
        let config = MemoryConfig::default();

        assert_eq!(config.soft_delete_recovery_days, 30);
        assert_eq!(config.hard_delete_importance, 0.1);
        assert_eq!(config.max_storage_gb, 10);
        assert_eq!(config.high_watermark, 0.9);
    }

    #[test]
    fn test_importance_config_defaults() {
        let config = ImportanceConfig::default();

        assert_eq!(config.lambda, 0.05);
        assert_eq!(config.weight_recency, 0.3);
    }

    #[test]
    fn test_dream_config_new_defaults() {
        let c = DreamConfig::default();
        // 上游去重与语义边候选限幅、图命中保留名额的默认值
        assert!(c.fusion_dedup_enabled);
        assert_eq!(c.fusion_dedup_threshold, 0.97);
        assert_eq!(c.networking_semantic_max_candidates, 20);
        assert_eq!(c.networking_semantic_dup_threshold, 0.97);
        assert_eq!(c.retrieval_graph_quota, 0.25);
        assert_eq!(c.retrieval_graph_weight, 0.3);
    }

    #[test]
    fn test_config_serde() {
        let config = MemoryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: MemoryConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            config.soft_delete_recovery_days,
            parsed.soft_delete_recovery_days
        );
    }

    #[test]
    fn test_source_weights_defaults() {
        let weights = SourceWeights::default();

        assert_eq!(weights.user, 1.0);
        assert_eq!(weights.system, 0.8);
        assert_eq!(weights.inferred, 0.5);
        assert_eq!(weights.external, 0.7);
    }

    #[test]
    fn test_source_weights_serde_roundtrip() {
        let weights = SourceWeights::default();
        let json = serde_json::to_string(&weights).unwrap();
        let parsed: SourceWeights = serde_json::from_str(&json).unwrap();

        assert_eq!(weights.user, parsed.user);
        assert_eq!(weights.system, parsed.system);
        assert_eq!(weights.inferred, parsed.inferred);
        assert_eq!(weights.external, parsed.external);
    }

    #[test]
    fn test_source_weights_partial_deserialize() {
        let json = r#"{"user": 2.0}"#;
        let parsed: SourceWeights = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.user, 2.0);
        assert_eq!(parsed.system, 0.8);
        assert_eq!(parsed.inferred, 0.5);
        assert_eq!(parsed.external, 0.7);
    }

    #[test]
    fn test_search_config_defaults() {
        let config = SearchConfig::default();

        assert_eq!(config.hybrid_alpha, 0.5);
        assert!(config.mmr_enabled);
        assert_eq!(config.mmr_lambda, 0.5);
        assert_eq!(config.decay_half_life_hours, 336.0);
        assert_eq!(config.source_weights.user, 1.0);
        assert_eq!(config.evergreen_scopes, vec!["global", "workspace"]);
    }

    #[test]
    fn test_search_config_serde_roundtrip() {
        let config = SearchConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: SearchConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.hybrid_alpha, parsed.hybrid_alpha);
        assert_eq!(config.mmr_enabled, parsed.mmr_enabled);
        assert_eq!(config.mmr_lambda, parsed.mmr_lambda);
        assert_eq!(config.decay_half_life_hours, parsed.decay_half_life_hours);
        assert_eq!(config.source_weights.user, parsed.source_weights.user);
        assert_eq!(config.evergreen_scopes, parsed.evergreen_scopes);
    }

    #[test]
    fn test_search_config_partial_deserialize() {
        let json = r#"{"hybrid_alpha": 0.8, "mmr_enabled": false}"#;
        let parsed: SearchConfig = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.hybrid_alpha, 0.8);
        assert!(!parsed.mmr_enabled);
        assert_eq!(parsed.mmr_lambda, 0.5);
        assert_eq!(parsed.decay_half_life_hours, 336.0);
        assert_eq!(parsed.source_weights.user, 1.0);
        assert_eq!(parsed.evergreen_scopes, vec!["global", "workspace"]);
    }

    #[test]
    fn test_dream_config_defaults() {
        let config = DreamConfig::default();

        assert!(!config.enabled);
        assert!(config.requires_llm);
        assert_eq!(config.min_memories, 20);
        assert_eq!(config.max_age_hours, 168);
        assert_eq!(config.min_hours_between, 24.0);
        assert_eq!(config.integration_type, "knowledge");
        assert_eq!(config.integration_tags, vec!["dream-integrated"]);
    }

    #[test]
    fn test_dream_config_integration_defaults() {
        let config = DreamConfig::default();

        assert_eq!(config.integration_max_memories, 100);
        assert_eq!(config.integration_preserve_importance, 0.85);
        assert_eq!(
            config.integration_preserve_tags,
            vec!["critical".to_string()]
        );
        assert_eq!(
            config.integration_preserve_types,
            vec!["preference".to_string(), "context".to_string()]
        );
        assert_eq!(config.integration_min_content_len, 10);
        assert_eq!(config.cleanup_inactive_days, 90);
        assert_eq!(config.cleanup_importance_threshold, 0.1);
    }

    #[test]
    fn test_dream_config_integration_serde() {
        let json = r#"{"enabled": true, "integration_max_memories": 50, "integration_preserve_importance": 0.9, "cleanup_inactive_days": 30}"#;
        let parsed: DreamConfig = serde_json::from_str(json).unwrap();

        assert!(parsed.enabled);
        assert_eq!(parsed.integration_max_memories, 50);
        assert_eq!(parsed.integration_preserve_importance, 0.9);
        assert_eq!(parsed.cleanup_inactive_days, 30);
        // 未指定的新字段走默认值
        assert_eq!(
            parsed.integration_preserve_tags,
            vec!["critical".to_string()]
        );
        assert_eq!(parsed.cleanup_importance_threshold, 0.1);
    }

    #[test]
    fn test_dream_config_serde_roundtrip() {
        let config = DreamConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: DreamConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.enabled, parsed.enabled);
        assert_eq!(config.min_memories, parsed.min_memories);
        assert_eq!(config.max_age_hours, parsed.max_age_hours);
        assert_eq!(config.min_hours_between, parsed.min_hours_between);
    }

    #[test]
    fn test_dream_config_partial_deserialize() {
        let json = r#"{"enabled": true, "min_memories": 50}"#;
        let parsed: DreamConfig = serde_json::from_str(json).unwrap();

        assert!(parsed.enabled);
        assert!(parsed.requires_llm);
        assert_eq!(parsed.min_memories, 50);
        assert_eq!(parsed.max_age_hours, 168);
        assert_eq!(parsed.min_hours_between, 24.0);
    }

    #[test]
    fn test_dream_config_new_fields() {
        let config = DreamConfig::default();

        assert!(config.phase_cross_project);
        assert!(config.phase_personal_summary);
        assert!(config.phase_cleanup);
        assert!(!config.phase_vector_regen);
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.batch_interval_ms, 1000);
    }

    #[test]
    fn test_dream_config_serde_new_fields() {
        let json = r#"{"enabled": true, "phase_cross_project": false, "batch_size": 50}"#;
        let parsed: DreamConfig = serde_json::from_str(json).unwrap();

        assert!(parsed.enabled);
        assert!(parsed.requires_llm);
        assert!(!parsed.phase_cross_project);
        assert!(parsed.phase_personal_summary);
        assert_eq!(parsed.batch_size, 50);
        assert_eq!(parsed.batch_interval_ms, 1000);
    }

    #[test]
    fn test_idle_monitor_config_defaults() {
        let config = IdleMonitorConfig::default();

        assert!(config.enabled);
        assert_eq!(config.check_interval_secs, 300);
        assert_eq!(config.load_threshold, 0.5);
        assert!(config.auto_dream_enabled);
    }

    #[test]
    fn test_idle_monitor_config_serde() {
        let config = IdleMonitorConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: IdleMonitorConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.check_interval_secs, parsed.check_interval_secs);
    }

    #[test]
    fn test_idle_monitor_config_partial_deserialize() {
        let json = r#"{"enabled": false, "load_threshold": 1.0}"#;
        let parsed: IdleMonitorConfig = serde_json::from_str(json).unwrap();

        assert!(!parsed.enabled);
        assert_eq!(parsed.load_threshold, 1.0);
        assert_eq!(parsed.check_interval_secs, 300);
    }

    #[test]
    fn test_queue_config_defaults() {
        let config = QueueConfig::default();

        assert_eq!(config.normal_queue_capacity, 1000);
        assert_eq!(config.dream_queue_capacity, 10);
    }

    #[test]
    fn test_queue_config_serde() {
        let json = r#"{"normal_queue_capacity": 500}"#;
        let parsed: QueueConfig = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.normal_queue_capacity, 500);
        assert_eq!(parsed.dream_queue_capacity, 10);
    }

    #[test]
    fn test_log_config_defaults() {
        let config = LogConfig::default();

        assert!(config.enabled);
        assert_eq!(config.level, "info");
        assert_eq!(config.rotation_days, 7);
        assert!(config.memrecd_log.contains("memrecd.log"));
    }

    #[test]
    fn test_log_config_serde() {
        let json = r#"{"rotation_days": 14, "level": "debug"}"#;
        let parsed: LogConfig = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.rotation_days, 14);
        assert_eq!(parsed.level, "debug");
        assert!(parsed.enabled);
    }
