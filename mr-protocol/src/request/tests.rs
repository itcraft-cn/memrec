//! request 类型单元测试（内联 tests 抽离）。

use super::*;

    #[test]
    fn test_request_creation() {
        let req = JsonRpcRequest::new(
            RequestAction::Add,
            Some(RequestParams::Add(AddParams {
                content: "test".to_string(),
                memory_type: MemoryType::Knowledge,
                tags: vec!["tag1".to_string()],
                project_id: None,
                is_global: false,
                working_dir: None,
                source: None,
                scope: None,
                dedup: None,
            })),
            1,
        );

        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, 1);
    }

    #[test]
    fn test_request_serde() {
        let req = JsonRpcRequest::new(
            RequestAction::Get,
            Some(RequestParams::Get(GetParams {
                id: Uuid::nil(),
                merge: false,
            })),
            1,
        );

        let json = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(req.jsonrpc, parsed.jsonrpc);
    }

    #[test]
    fn test_search_params_defaults() {
        let params = SearchParams {
            text: None,
            mode: SearchMode::default(),
            tags: Vec::new(),
            time_range: None,
            project_id: None,
            top_k: default_top_k(),
            min_importance: 0.0,
        };

        assert_eq!(params.mode, SearchMode::Hybrid);
        assert_eq!(params.top_k, 10);
    }

    #[test]
    fn test_search_memory_params_defaults() {
        let params = SearchMemoryParams {
            query: "test".to_string(),
            project_id: None,
            include_global: default_include_global(),
            project_only: false,
            global_only: false,
            cross_project: false,
            memory_type: None,
            top_k: default_top_k(),
            min_score: default_min_score(),
            working_dir: None,
            hybrid_alpha: default_hybrid_alpha(),
            mmr_enabled: default_mmr_enabled(),
            mmr_lambda: default_mmr_lambda(),
        };

        assert!(params.include_global);
        assert_eq!(params.top_k, 10);
        assert_eq!(params.min_score, 0.5);
    }

    #[test]
    fn test_search_memory_params_serde() {
        let params = SearchMemoryParams {
            query: "authentication".to_string(),
            project_id: Some(Uuid::new_v4()),
            include_global: true,
            project_only: false,
            global_only: false,
            cross_project: false,
            memory_type: Some(MemoryType::Decision),
            top_k: 20,
            min_score: 0.8,
            working_dir: None,
            hybrid_alpha: default_hybrid_alpha(),
            mmr_enabled: default_mmr_enabled(),
            mmr_lambda: default_mmr_lambda(),
        };

        let json = serde_json::to_string(&params).unwrap();
        let parsed: SearchMemoryParams = serde_json::from_str(&json).unwrap();

        assert_eq!(params.query, parsed.query);
        assert_eq!(params.top_k, parsed.top_k);
    }

    #[test]
    fn test_add_params_with_source_scope() {
        let json = r#"{
            "type": "add",
            "content": "test content",
            "source": "system",
            "scope": "global"
        }"#;

        let params: RequestParams = serde_json::from_str(json).unwrap();
        if let RequestParams::Add(p) = params {
            assert_eq!(p.source, Some("system".to_string()));
            assert_eq!(p.scope, Some("global".to_string()));
        } else {
            panic!("Expected Add params");
        }
    }

    #[test]
    fn test_search_memory_params_hybrid() {
        let json = r#"{
            "type": "search_memory",
            "query": "test query",
            "hybrid_alpha": 0.7,
            "mmr_enabled": false
        }"#;

        let params: RequestParams = serde_json::from_str(json).unwrap();
        if let RequestParams::SearchMemory(p) = params {
            assert_eq!(p.hybrid_alpha, 0.7);
            assert!(!p.mmr_enabled);
        } else {
            panic!("Expected SearchMemory params");
        }
    }
