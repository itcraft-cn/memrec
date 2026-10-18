//! McpServer 单元测试（内联 tests 抽离）。

use super::*;


    fn make_server() -> McpServer {
        McpServer::new()
    }

    #[test]
    fn test_handle_initialize() {
        let server = make_server();
        let result = server.handle_initialize(Some(1.into()), &json!({}));

        assert_eq!(result["jsonrpc"], "2.0");
        assert_eq!(result["id"], 1);
        assert!(result["result"]["protocolVersion"].is_string());
        assert!(result["result"]["capabilities"]["tools"].is_object());
        assert!(result["result"]["serverInfo"]["name"].is_string());
    }

    #[test]
    fn test_handle_tools_list() {
        let server = make_server();
        let result = server.handle_tools_list(Some(2.into()));

        assert!(result["result"]["tools"].is_array());
        let tools = result["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 6);

        let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(tool_names.contains(&"mr_add"));
        assert!(tool_names.contains(&"mr_search"));
        assert!(tool_names.contains(&"mr_get"));
        assert!(tool_names.contains(&"mr_list"));
        assert!(tool_names.contains(&"mr_delete"));
        assert!(tool_names.contains(&"mr_stats"));
    }

    #[test]
    fn test_handle_resources_list() {
        let server = make_server();
        let result = server.handle_resources_list(Some(3.into()));

        let resources = result["result"]["resources"].as_array().unwrap();
        assert!(resources.len() >= 2);

        let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
        assert!(uris.contains(&"memrec://stats"));
        assert!(uris.contains(&"memrec://project"));
    }

    #[test]
    fn test_make_result() {
        let server = make_server();
        let result = server.make_result(Some(42.into()), json!({"key": "value"}));

        assert_eq!(result["jsonrpc"], "2.0");
        assert_eq!(result["id"], 42);
        assert_eq!(result["result"]["key"], "value");
    }

    #[test]
    fn test_make_error() {
        let server = make_server();
        let result = server.make_error(Some(99.into()), -32600, "Invalid Request");

        assert_eq!(result["jsonrpc"], "2.0");
        assert_eq!(result["id"], 99);
        assert_eq!(result["error"]["code"], -32600);
        assert_eq!(result["error"]["message"], "Invalid Request");
    }

    #[test]
    fn test_tool_schema_validation() {
        let server = make_server();
        let result = server.handle_tools_list(Some(1.into()));

        for tool in result["result"]["tools"].as_array().unwrap() {
            assert!(tool["name"].is_string());
            assert!(tool["description"].is_string());
            assert!(tool["inputSchema"].is_object());
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn test_mr_add_tool_schema() {
        let server = make_server();
        let result = server.handle_tools_list(Some(1.into()));

        let mr_add = result["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "mr_add")
            .unwrap();

        let required = mr_add["inputSchema"]["required"].as_array().unwrap();
        assert!(required.contains(&json!("content")));
        assert!(required.contains(&json!("memory_type")));
    }

    #[test]
    fn test_mr_search_tool_schema() {
        let server = make_server();
        let result = server.handle_tools_list(Some(1.into()));

        let mr_search = result["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "mr_search")
            .unwrap();

        let required = mr_search["inputSchema"]["required"].as_array().unwrap();
        assert!(required.contains(&json!("query")));

        // scope 三态枚举：与 CLI -p/-g/-a 对齐
        let scope = &mr_search["inputSchema"]["properties"]["scope"];
        let enum_values: Vec<&str> = scope["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(enum_values, vec!["project", "global", "all"]);
    }

    #[test]
    fn test_mr_list_tool_schema_scope() {
        let server = make_server();
        let result = server.handle_tools_list(Some(1.into()));

        let mr_list = result["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "mr_list")
            .unwrap();

        let scope = &mr_list["inputSchema"]["properties"]["scope"];
        let enum_values: Vec<&str> = scope["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(enum_values, vec!["project", "global", "all"]);
        // 旧布尔参数已移除，仅保留 scope
        assert!(mr_list["inputSchema"]["properties"]
            .get("project_only")
            .is_none());
        assert!(mr_list["inputSchema"]["properties"]
            .get("global_only")
            .is_none());
    }

    #[test]
    fn test_parse_scope_explicit() {
        assert_eq!(parse_scope(Some("project")), Scope::Project);
        assert_eq!(parse_scope(Some("global")), Scope::Global);
        assert_eq!(parse_scope(Some("all")), Scope::All);
    }

    #[test]
    fn test_parse_scope_invalid_falls_back_to_default() {
        // 未知值/缺失时回退默认推断（当前工作目录有 .mr_pid 时为 Project）
        let default = default_scope();
        assert_eq!(parse_scope(Some("invalid")), default);
        assert_eq!(parse_scope(None), default);
    }

    #[test]
    fn test_add_working_dir_global_is_none() {
        // 全局写入不携带工作目录，归属由 is_global 决定为全局。
        assert_eq!(add_working_dir(true), None);
    }

    #[test]
    fn test_add_working_dir_project_uses_caller_cwd() {
        // 回归：非全局写入必须携带调用方工作目录，否则守护进程回落到自身 cwd，
        // 所有 MCP 写入汇入同一项目桶（项目隔离失效）。
        let wd = add_working_dir(false);
        assert!(wd.is_some(), "非全局写入必须携带工作目录");
        assert_eq!(wd, detect_working_dir().ok());
    }
