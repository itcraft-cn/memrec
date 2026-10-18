//! DreamProcessor 单元测试（原处理器内联 tests 抽离）。

    use super::*;
    use super::helpers::*;
    use std::collections::HashMap;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    /// T5 验证：DreamError 经 #[from] 保留原始错误链，Display 输出包裹层
    /// 与根因两层信息（旧 String 化实现会丢根因）。
    #[test]
    fn test_dream_error_preserves_chain() {
        let inner = anyhow::anyhow!("inner root cause").context("storage layer ctx");
        let display = DreamError::from(inner).to_string();
        assert!(display.contains("Storage error: storage layer ctx"));
        assert!(display.contains("inner root cause"));

        let llm_err = DreamError::Llm(LlmError::EmptyResponse).to_string();
        assert!(llm_err.contains("LLM error: LLM response is empty"));
    }

    fn create_test_processor() -> (DreamProcessor, Arc<MemoryStore>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            min_hours_between: 0.0,
            min_memories: 2,
            max_age_hours: 1,
            requires_llm: false,
            ..Default::default()
        };

        let processor = DreamProcessor::new(config, dir.path(), storage.clone());
        (processor, storage, dir)
    }

    #[tokio::test]
    async fn test_execute_disabled() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: false,
            ..Default::default()
        };

        let processor = DreamProcessor::new(config, dir.path(), storage);

        let result = processor.execute(false).await;
        assert!(matches!(result, Err(DreamError::GateSkipped { .. })));
    }

    #[tokio::test]
    async fn test_execute_not_enough_memories() {
        let (processor, storage, _dir) = create_test_processor();

        let mut memory = Memory::new("single memory".to_string(), MemoryType::Knowledge);
        memory.created_at = Utc::now() - chrono::Duration::hours(2);
        storage.save(&memory).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 0);
        assert!(dream_result.created_memory_id.is_none());
    }

    #[tokio::test]
    async fn test_execute_success() {
        let (processor, storage, _dir) = create_test_processor();

        for i in 0..3 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            memory.created_at = Utc::now() - chrono::Duration::hours(2);
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_ok());

        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 3);
        assert!(dream_result.created_memory_id.is_some());
        assert!(!dream_result.summary.is_empty());
        assert!(!dream_result.phase_results.is_empty());
    }

    #[test]
    fn test_clean_summary_strips_code_fence() {
        let raw = "```markdown\n## 主题\nRust 异步性能优化\n```";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "## 主题\nRust 异步性能优化");
    }

    #[test]
    fn test_clean_summary_strips_small_talk_prefix() {
        let raw = "好的，以下是整合后的摘要：\n\n## 主题\n认证方案选型";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "## 主题\n认证方案选型");
    }

    #[test]
    fn test_clean_summary_strips_nested_prefixes() {
        let raw = "好的，以下为总结如下：项目采用 JWT 认证";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "项目采用 JWT 认证");
    }

    #[test]
    fn test_clean_summary_keeps_content() {
        let raw = "## 要点\n- JWT 无状态易扩展";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, raw);
    }

    #[test]
    fn test_clean_summary_compacts_blank_lines() {
        let raw = "## 主题\n\n\n\n## 要点\n- a\n\n\n- b";
        let cleaned = clean_summary(raw);
        assert_eq!(cleaned, "## 主题\n\n## 要点\n- a\n\n- b");
    }

    #[test]
    fn test_dream_result_debug() {
        let result = DreamResult {
            run_id: None,
            integrated_count: 5,
            created_memory_id: Some(Uuid::nil()),
            summary: "test summary".to_string(),
            summary_clues: SummaryClues::default(),
            phase_results: vec![],
            cluster_count: 1,
            contradiction_count: 0,
            edge_count: 0,
            propagation_count: 0,
            dry_run: false,
            metrics: QualityMetrics::default(),
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("integrated_count"));
        assert!(debug_str.contains("5"));
    }

    #[tokio::test]
    async fn test_execute_requires_llm_without_llm_fails() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: true,
            ..Default::default()
        };

        let processor = DreamProcessor::new(config, dir.path(), storage);

        let result = processor.execute(true).await;
        assert!(matches!(result, Err(DreamError::GateFailed(_))));
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("LLM"));
    }

    #[tokio::test]
    async fn test_execute_with_llm_uses_llm_summary() {
        use crate::llm::MockLlmClient;
        use std::sync::Arc as StdArc;

        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let llm = MockLlmClient::new(
            r#"{"facts":[{"text":"fact from llm"}],"summary":"LLM 整合摘要内容","contradictions":[]}"#,
        );

        let config = DreamConfig {
            enabled: true,
            requires_llm: true,
            min_hours_between: 0.0,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };

        let processor =
            DreamProcessor::new(config, dir.path(), storage.clone()).with_llm(StdArc::new(llm));

        for i in 0..3 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            // 记忆年龄须大于 max_age_hours（100h），才会被选中
            memory.created_at = Utc::now() - chrono::Duration::hours(200);
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 3);
        assert_eq!(dream_result.summary, "LLM 整合摘要内容");
        assert!(dream_result.created_memory_id.is_some());
    }

    #[tokio::test]
    async fn test_execute_failure_does_not_write_state() {
        // P2/P6: LLM 失败时 execute 返回 Err；不写 state（不消耗 24h 重试窗口），
        // 且 phases 不执行（无 cross-project 摘要写入）。
        use crate::llm::MockLlmClient;
        use std::sync::Arc as StdArc;

        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        // unconfigured Mock：chat 返回 Err
        let llm = MockLlmClient::unconfigured();

        let config = DreamConfig {
            enabled: true,
            requires_llm: true,
            min_memories: 1,
            max_age_hours: 100,
            ..Default::default()
        };

        let processor =
            DreamProcessor::new(config, dir.path(), storage.clone()).with_llm(StdArc::new(llm));

        for i in 0..3 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            memory.created_at = Utc::now() - chrono::Duration::hours(200);
            memory.importance = 0.5;
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_err());

        // P2: 失败不写 state → 下次仍可立即重试
        let lock = crate::dream::DreamLock::new(dir.path());
        assert!(lock.read_state().is_none());

        // P6: phases 未执行，无 cross-project 摘要
        let summaries = storage.list_by_tag("cross-project", 10).await.unwrap();
        assert!(summaries.is_empty());
    }

    #[tokio::test]
    async fn test_integration_preserves_high_value_memories() {
        // P4: critical 标签 / 高 importance / preference 类型不参与整合与删除
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        // 普通旧记忆：应被整合
        let mut normal = Memory::new("normal old memory".to_string(), MemoryType::Knowledge);
        normal.created_at = Utc::now() - chrono::Duration::hours(200);
        normal.importance = 0.5;
        storage.save(&normal).await.unwrap();

        // critical 标签记忆：应保留
        let mut critical = Memory::new("critical decision".to_string(), MemoryType::Decision);
        critical.created_at = Utc::now() - chrono::Duration::hours(200);
        critical.importance = 0.5;
        critical.tags = vec!["critical".to_string()];
        storage.save(&critical).await.unwrap();

        // 高 importance 记忆：应保留
        let mut high = Memory::new("high importance".to_string(), MemoryType::Knowledge);
        high.created_at = Utc::now() - chrono::Duration::hours(200);
        high.importance = 0.9;
        storage.save(&high).await.unwrap();

        // preference 类型：应保留
        let mut pref = Memory::new("user preference".to_string(), MemoryType::Preference);
        pref.created_at = Utc::now() - chrono::Duration::hours(200);
        pref.importance = 0.5;
        storage.save(&pref).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 1);

        // 被保留的记忆仍在库中（未软删）
        let all = storage.list(100).await.unwrap();
        let alive: Vec<_> = all.into_iter().filter(|m| !m.is_deleted).collect();
        assert!(alive.iter().any(|m| m.id == critical.id));
        assert!(alive.iter().any(|m| m.id == high.id));
        assert!(alive.iter().any(|m| m.id == pref.id));
        assert!(!alive.iter().any(|m| m.id == normal.id));
    }

    #[tokio::test]
    async fn test_integration_skips_already_integrated_sources() {
        // P5: 已整合源记忆（metadata.source_ids 命中）被排除，且残留被补删
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        // 模拟上次崩溃残留：源记忆未被删除，但已有整合记忆记录了它
        let mut leftover = Memory::new("leftover source".to_string(), MemoryType::Knowledge);
        leftover.created_at = Utc::now() - chrono::Duration::hours(200);
        leftover.importance = 0.5;
        storage.save(&leftover).await.unwrap();

        let mut integrated = Memory::new("previous summary".to_string(), MemoryType::Knowledge);
        integrated.tags = vec!["dream-integrated".to_string()];
        integrated.created_at = Utc::now() - chrono::Duration::hours(50);
        let mut md = HashMap::new();
        md.insert(
            "source_ids".to_string(),
            serde_json::to_string(&vec![leftover.id.to_string()]).unwrap(),
        );
        integrated.metadata = md;
        storage.save(&integrated).await.unwrap();

        // 再加一条真正的新旧记忆，保证候选数达到 min_memories
        let mut fresh = Memory::new("fresh candidate".to_string(), MemoryType::Knowledge);
        fresh.created_at = Utc::now() - chrono::Duration::hours(200);
        fresh.importance = 0.5;
        storage.save(&fresh).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        // 仅整合 fresh（leftover 已被排除并补删）
        assert_eq!(dream_result.integrated_count, 1);

        let all = storage.list(100).await.unwrap();
        let alive: Vec<_> = all.into_iter().filter(|m| !m.is_deleted).collect();
        assert!(!alive.iter().any(|m| m.id == leftover.id));
        assert!(!alive.iter().any(|m| m.id == fresh.id));
    }

    #[tokio::test]
    async fn test_integration_preserves_context_and_short_content() {
        // P4 扩展: context 类型（配置/环境信息）默认保留；过短内容（测试/占位）不整合
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        // 普通旧知识：应整合
        let mut knowledge = Memory::new(
            "regular old knowledge about glibc arena".to_string(),
            MemoryType::Knowledge,
        );
        knowledge.created_at = Utc::now() - chrono::Duration::hours(200);
        knowledge.importance = 0.5;
        storage.save(&knowledge).await.unwrap();

        // context 类型：应保留（配置细节不可压缩）
        let mut ctx = Memory::new(
            "cotael-srv 配置路径: scenes.toml 账目体系, scheduler.toml 任务调度".to_string(),
            MemoryType::Context,
        );
        ctx.created_at = Utc::now() - chrono::Duration::hours(200);
        ctx.importance = 0.5;
        storage.save(&ctx).await.unwrap();

        // 过短内容（测试垃圾）：不参与整合，保留在库中
        let mut junk = Memory::new("测试".to_string(), MemoryType::Knowledge);
        junk.created_at = Utc::now() - chrono::Duration::hours(200);
        junk.importance = 0.5;
        storage.save(&junk).await.unwrap();

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        // 仅整合 knowledge（context 与过短内容被排除）
        assert_eq!(dream_result.integrated_count, 1);

        let all = storage.list(100).await.unwrap();
        let alive: Vec<_> = all.into_iter().filter(|m| !m.is_deleted).collect();
        assert!(alive.iter().any(|m| m.id == ctx.id), "context 应保留");
        assert!(alive.iter().any(|m| m.id == junk.id), "过短内容应保留");
        assert!(!alive.iter().any(|m| m.id == knowledge.id));
    }

    #[tokio::test]
    async fn test_integration_respects_max_memories() {
        // P1: integration_max_memories 限制单次整合输入量
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));

        let config = DreamConfig {
            enabled: true,
            requires_llm: false,
            min_memories: 1,
            max_age_hours: 100,
            integration_max_memories: 2,
            phase_cross_project: false,
            phase_personal_summary: false,
            phase_cleanup: false,
            ..Default::default()
        };
        let processor = DreamProcessor::new(config, dir.path(), storage.clone());

        for i in 0..5 {
            let mut memory = Memory::new(
                format!("memory {} content for integration test", i),
                MemoryType::Knowledge,
            );
            memory.created_at = Utc::now() - chrono::Duration::hours(200);
            memory.importance = 0.5;
            storage.save(&memory).await.unwrap();
        }

        let result = processor.execute(true).await;
        assert!(result.is_ok());
        let dream_result = result.unwrap();
        assert_eq!(dream_result.integrated_count, 2);
    }

    #[test]
    fn test_cosine_similarity_basic() {
        // 同向量相似度为 1，正交为 0，维度不一致或零向量为 0
        assert!((super::helpers::cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!((super::helpers::cosine_similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
        assert_eq!(super::helpers::cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(super::helpers::cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        // 近重复向量相似度应高于去重阈值 0.97
        let a = [1.0f32, 2.0, 3.0, 4.0];
        let b = [1.0f32, 2.0, 3.0, 4.01];
        assert!(super::helpers::cosine_similarity(&a, &b) >= 0.97);
    }
