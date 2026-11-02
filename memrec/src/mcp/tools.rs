//! MCP 工具定义与调用处理。

use super::*;

impl McpServer {
    /// 处理 `tools/list` 方法，返回可用工具的 schema。
    pub(super) fn handle_tools_list(&self, id: Option<Value>) -> Value {
        self.make_result(id, json!({
            "tools": [
                {
                    "name": "mr_add",
                    "description": "Add a memory to MemRec. Supports project isolation and global memories.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "Memory content" },
                            "memory_type": {
                                "type": "string",
                                "enum": ["decision", "knowledge", "context", "preference", "conversation"],
                                "description": "Memory type. decision=key decisions, knowledge=learnings/facts, context=project info, preference=user prefs, conversation=dialogue"
                            },
                            "tags": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Tags for categorization. Use 'critical' for important, 'fact' for objective facts, 'best-practice' for patterns"
                            },
                            "is_global": { "type": "boolean", "description": "If true, memory is accessible from all projects (use for user preferences)" }
                        },
                        "required": ["content", "memory_type"]
                    }
                },
                {
                    "name": "mr_search",
                    "description": "Search memories using semantic similarity. Returns results scored by relevance (0-1). Scope: 'project'=current project only, 'global'=global memories only, 'all'=across all projects. When scope is omitted it is inferred from the .mr_pid marker in the project root (project if present, else global).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search query" },
                            "top_k": { "type": "integer", "description": "Max results to return", "default": 10 },
                            "min_score": { "type": "number", "description": "Minimum similarity score (0-1)", "default": 0.5 },
                            "scope": {
                                "type": "string",
                                "enum": ["project", "global", "all"],
                                "description": "Search scope. Default: inferred from .mr_pid (project if present, else global)"
                            },
                            "memory_type": {
                                "type": "string",
                                "enum": ["decision", "knowledge", "context", "preference", "conversation"],
                                "description": "Filter by memory type"
                            }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "mr_get",
                    "description": "Get a specific memory by ID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Memory UUID" },
                            "merge": { "type": "boolean", "description": "Merge chunked memories into full content" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "mr_list",
                    "description": "List memories with optional filtering. Scope: 'project'=current project only, 'global'=global memories only, 'all'=all projects. When scope is omitted it is inferred from the .mr_pid marker in the project root (project if present, else global). Set raw=true to read raw records directly from storage, including sources soft-deleted by Dream fusion; raw excludes fusion outputs by default, set with_dream=true to include them. Uses storage order.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max memories to return", "default": 20 },
                            "skip": { "type": "integer", "description": "Number of records to skip for pagination", "default": 0 },
                            "scope": {
                                "type": "string",
                                "enum": ["project", "global", "all"],
                                "description": "List scope. Default: inferred from .mr_pid (project if present, else global)"
                            },
                            "tags": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Filter by tags; all tags must match"
                            },
                            "memory_type": {
                                "type": "string",
                                "enum": ["decision", "knowledge", "context", "preference", "conversation"],
                                "description": "Filter by memory type"
                            },
                            "include_deleted": { "type": "boolean", "description": "Return only soft-deleted records", "default": false },
                            "raw": { "type": "boolean", "description": "Raw record mode: read storage directly and keep records soft-deleted by Dream fusion", "default": false },
                            "with_dream": { "type": "boolean", "description": "In raw mode, also include Dream fusion outputs (metadata.source_ids); ignored otherwise", "default": false }
                        }
                    }
                },
                {
                    "name": "mr_timeline",
                    "description": "Browse memories by creation time (newest first) with pagination and filtering. Same scope/filter semantics as mr_list. Set raw=true to read raw records directly from storage, including sources soft-deleted by Dream fusion; raw excludes fusion outputs by default, set with_dream=true to include them.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max memories to return", "default": 20 },
                            "skip": { "type": "integer", "description": "Number of records to skip for pagination", "default": 0 },
                            "scope": {
                                "type": "string",
                                "enum": ["project", "global", "all"],
                                "description": "Timeline scope. Default: inferred from .mr_pid (project if present, else global)"
                            },
                            "tags": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Filter by tags; all tags must match"
                            },
                            "memory_type": {
                                "type": "string",
                                "enum": ["decision", "knowledge", "context", "preference", "conversation"],
                                "description": "Filter by memory type"
                            },
                            "raw": { "type": "boolean", "description": "Raw record mode: read storage directly and keep records soft-deleted by Dream fusion", "default": false },
                            "with_dream": { "type": "boolean", "description": "In raw mode, also include Dream fusion outputs (metadata.source_ids); ignored otherwise", "default": false }
                        }
                    }
                },
                {
                    "name": "mr_delete",
                    "description": "Delete a memory by ID (soft delete).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Memory UUID" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "mr_stats",
                    "description": "Get memory statistics.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mr_dream",
                    "description": "Execute Dream memory integration. Consolidates and compresses older memories into summary memories. Bypasses time/session gates when force=true. Set dry_run=true to preview without writing.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "force": { "type": "boolean", "description": "If true, bypass gate checks (time interval, session count)", "default": false },
                            "dry_run": { "type": "boolean", "description": "If true, preview only: no writes, no deletes, no edges", "default": false }
                        }
                    }
                },
                {
                    "name": "mr_dream_report",
                    "description": "Get recent Dream run audit reports (DreamRun): candidates, clusters, edges, propagations, gate outcome, status and quality metrics.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "last": { "type": "integer", "description": "Number of recent runs to return (default 10, max 100)", "default": 10 }
                        }
                    }
                }
            ]
        }))
    }

    /// 处理 `tools/call` 方法，分发到具体工具实现。
    pub(super) async fn handle_tools_call(&self, id: Option<Value>, params: &Value) -> Value {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match tool_name {
            "mr_add" => self.tool_add(&arguments).await,
            "mr_search" => self.tool_search(&arguments).await,
            "mr_get" => self.tool_get(&arguments).await,
            "mr_list" => self.tool_list(&arguments).await,
            "mr_timeline" => self.tool_timeline(&arguments).await,
            "mr_delete" => self.tool_delete(&arguments).await,
            "mr_stats" => self.tool_stats().await,
            "mr_dream" => self.tool_dream(&arguments).await,
            "mr_dream_report" => self.tool_dream_report(&arguments).await,
            _ => Err(format!("Unknown tool: {}", tool_name)),
        };

        match result {
            Ok(content) => self.make_result(
                id,
                json!({
                    "content": [{"type": "text", "text": content}],
                    "isError": false
                }),
            ),
            Err(msg) => self.make_result(
                id,
                json!({
                    "content": [{"type": "text", "text": msg}],
                    "isError": true
                }),
            ),
        }
    }

    /// `mr_add` 工具实现：添加记忆。
    ///
    /// 与 `mr_search` / CLI `add` 对齐：非全局记忆传递调用方工作目录，供守护进程
    /// 正确定位项目 UUID。此前恒传 `working_dir: None`，守护进程会回落到自身 cwd，
    /// 导致所有 MCP 写入汇入同一项目桶，项目内检索无法命中。
    async fn tool_add(&self, args: &Value) -> Result<String, String> {
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let memory_type = args
            .get("memory_type")
            .and_then(|v| v.as_str())
            .unwrap_or("conversation");
        let tags: Vec<String> = args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let is_global = args
            .get("is_global")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::Add,
                Some(RequestParams::Add(AddParams {
                    content: content.to_string(),
                    memory_type: parse_memory_type(memory_type),
                    tags,
                    project_id: None,
                    is_global,
                    working_dir: add_working_dir(is_global),
                    source: None,
                    scope: None,
                    dedup: None,
                })),
                1,
            ))
            .await?;

        let memory_id = resp
            .get("result")
            .and_then(|r| r.get("memory"))
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Ok(format!(
            "Added memory: {} (type: {}, global: {})",
            memory_id, memory_type, is_global
        ))
    }

    /// `mr_search` 工具实现：语义搜索。
    ///
    /// `scope` 三态参数（project/global/all）与 CLI 的 `-p/-g/-a` 对齐；
    /// 未指定时按项目根 `.mr_pid` 是否存在推断（存在→project，不存在→global）。
    /// project 范围时携带 `working_dir`，供守护进程正确定位项目 UUID。
    async fn tool_search(&self, args: &Value) -> Result<String, String> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let top_k = args.get("top_k").and_then(|v| v.as_u64()).unwrap_or(10);
        let min_score = args
            .get("min_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        let scope = parse_scope(args.get("scope").and_then(|v| v.as_str()));
        let memory_type = args.get("memory_type").and_then(|v| v.as_str());

        let (project_only, global_only, cross_project) = match scope {
            Scope::Project => (true, false, false),
            Scope::Global => (false, true, false),
            Scope::All => (false, false, true),
        };

        // project 范围：传递 working_dir，避免守护进程用自身 cwd 定位到错误项目
        let working_dir = if scope == Scope::Project {
            detect_working_dir().ok()
        } else {
            None
        };

        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::SearchMemory,
                Some(RequestParams::SearchMemory(SearchMemoryParams {
                    query: query.to_string(),
                    project_id: None,
                    include_global: false,
                    project_only,
                    global_only,
                    cross_project,
                    memory_type: memory_type
                        .filter(|s| !s.is_empty())
                        .map(parse_memory_type),
                    top_k: top_k as usize,
                    min_score: min_score as f32,
                    working_dir,
                    hybrid_alpha: mr_protocol::default_hybrid_alpha(),
                    mmr_enabled: mr_protocol::default_mmr_enabled(),
                    mmr_lambda: mr_protocol::default_mmr_lambda(),
                })),
                1,
            ))
            .await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    /// `mr_get` 工具实现：获取记忆。
    async fn tool_get(&self, args: &Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let merge = args.get("merge").and_then(|v| v.as_bool()).unwrap_or(false);

        let id = uuid::Uuid::parse_str(id).map_err(|_| format!("Invalid memory id: {}", id))?;
        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::Get,
                Some(RequestParams::Get(GetParams { id, merge })),
                1,
            ))
            .await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    /// `mr_list` 工具实现：按存储顺序列出记忆。
    ///
    /// `scope` 三态参数与 CLI 的 `-p/-g/-a` 对齐；未指定时按 `.mr_pid` 推断。
    /// `raw=true` 时直读存储层并保留被 Dream 融合软删的记录。
    async fn tool_list(&self, args: &Value) -> Result<String, String> {
        self.list_common(args, ListOrder::Storage).await
    }

    /// `mr_timeline` 工具实现：按创建时间倒序列出记忆。
    ///
    /// 参数与 `mr_list` 对齐，排序固定为 `created_desc`，供 AI 回看进展；
    /// 同样支持 `raw` / `with_dream` 原始记录视图。
    async fn tool_timeline(&self, args: &Value) -> Result<String, String> {
        self.list_common(args, ListOrder::CreatedDesc).await
    }

    /// `mr_list` / `mr_timeline` 共享实现：构造 List 请求并调用守护进程。
    async fn list_common(&self, args: &Value, order: ListOrder) -> Result<String, String> {
        let params = Self::build_list_params(args, order)?;
        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(params)),
                1,
            ))
            .await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    /// 解析 `list`/`timeline` 的公共参数。
    ///
    /// `order` 由调用方指定：`list` 用存储顺序，`timeline` 用创建时间倒序。
    /// project 范围解析 `.mr_pid` 得到项目 UUID，精确限定当前项目；
    /// `raw`/`with_dream`/`include_deleted` 直接透传给服务端。
    pub(super) fn build_list_params(args: &Value, order: ListOrder) -> Result<ListParams, String> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);
        let skip = args.get("skip").and_then(|v| v.as_u64()).unwrap_or(0);
        let scope = parse_scope(args.get("scope").and_then(|v| v.as_str()));
        let tags: Vec<String> = args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let memory_type = args
            .get("memory_type")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(parse_memory_type);
        let include_deleted = args
            .get("include_deleted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let raw = args.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);
        let with_dream = args
            .get("with_dream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let (project_only, global_only, project_id) = match scope {
            Scope::Project => {
                let root = detect_working_dir().map_err(|e| e.to_string())?;
                let root = std::path::PathBuf::from(root);
                (true, false, read_project_uuid(&root))
            }
            Scope::Global => (false, true, None),
            Scope::All => (false, false, None),
        };

        Ok(ListParams {
            tags,
            memory_type,
            limit: limit as usize,
            project_only,
            global_only,
            project_id,
            include_deleted,
            skip: skip as usize,
            order,
            raw,
            with_dream,
        })
    }

    /// `mr_delete` 工具实现：删除记忆。
    async fn tool_delete(&self, args: &Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");

        let id = uuid::Uuid::parse_str(id).map_err(|_| format!("Invalid memory id: {}", id))?;
        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::Delete,
                Some(RequestParams::Delete(DeleteParams { id, force: false })),
                1,
            ))
            .await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    /// `mr_stats` 工具实现：统计信息。
    async fn tool_stats(&self) -> Result<String, String> {
        let resp = self
            .call_daemon(JsonRpcRequest::new(RequestAction::Stats, None, 1))
            .await?;
        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    /// `mr_dream` 工具实现：记忆整合。
    async fn tool_dream(&self, args: &Value) -> Result<String, String> {
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::Dream,
                Some(RequestParams::Dream(DreamParams { force, dry_run })),
                1,
            ))
            .await?;

        if let Some(result) = resp.get("result") {
            if let Some(d) = result.get("integrated_count") {
                let count = d.as_u64().unwrap_or(0);
                let summary = result.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                let clusters = result
                    .get("cluster_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let edges = result
                    .get("edge_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let propagations = result
                    .get("propagation_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let mode = if dry_run {
                    "Dream dry-run"
                } else {
                    "Dream completed"
                };
                return Ok(format!(
                    "{}: {} memories integrated, {} clusters, {} edges, {} propagations\nSummary: {}",
                    mode, count, clusters, edges, propagations, summary
                ));
            }
        }

        if let Some(err) = resp.get("error") {
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            // daemon 侧错误串已带类别前缀（Dream skipped / Gate check failed / LLM error ...），
            // 直接透传，避免与"Dream failed"叠加成双重前缀。
            return Err(msg.to_string());
        }

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    /// `mr_dream_report` 工具实现：返回最近的 DreamRun 审计报告。
    async fn tool_dream_report(&self, args: &Value) -> Result<String, String> {
        let last = args.get("last").and_then(|v| v.as_u64()).unwrap_or(10);
        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::DreamReport,
                Some(RequestParams::DreamReport(DreamReportParams {
                    last: Some(last as usize),
                    offset: None,
                })),
                1,
            ))
            .await?;

        if let Some(result) = resp.get("result") {
            if let Some(runs) = result.get("runs") {
                return Ok(serde_json::to_string_pretty(runs).unwrap_or_default());
            }
        }

        if let Some(err) = resp.get("error") {
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(format!("Dream report failed: {}", msg));
        }

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

}
