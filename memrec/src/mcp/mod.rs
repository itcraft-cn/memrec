//! # MCP（Model Context Protocol）服务器
//!
//! 通过 stdin/stdout 与 AI 工具通信的 JSON-RPC 服务器，
//! 将 MCP 请求翻译为 memrecd 守护进程调用。
//!
//! ## 支持的 MCP 方法
//!
//! - `initialize`：协议握手
//! - `tools/list`：列出可用工具（mr_add, mr_search, mr_get, mr_list, mr_delete, mr_stats, mr_dream）
//! - `tools/call`：调用工具
//! - `resources/list`：列出资源（stats, project）
//! - `resources/read`：读取资源
//! - `ping`：心跳
//!
//! ## 工具列表
//!
//! | 工具 | 说明 |
//! |------|------|
//! | `mr_add` | 添加记忆 |
//! | `mr_search` | 语义搜索 |
//! | `mr_get` | 获取记忆 |
//! | `mr_list` | 列出记忆 |
//! | `mr_delete` | 删除记忆 |
//! | `mr_stats` | 统计信息 |
//! | `mr_dream` | 记忆整合 |

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use mr_protocol::{
    AddParams, DeleteParams, DreamParams, DreamReportParams, GetParams, GetProjectInfoParams,
    JsonRpcRequest, ListParams, RequestAction, RequestParams, SearchMemoryParams,
};

use crate::commands::{default_scope, detect_working_dir, read_project_uuid, Scope};

/// 解析 MCP 工具的 `scope` 参数（project/global/all）。
///
/// 与 CLI 的 `-p/-g/-a` 对齐：显式值优先，未指定时按项目根 `.mr_pid`
/// 是否存在推断默认范围（存在→project，不存在→global）。
fn parse_scope(scope: Option<&str>) -> Scope {
    match scope {
        Some("project") => Scope::Project,
        Some("global") => Scope::Global,
        Some("all") => Scope::All,
        _ => default_scope(),
    }
}

/// 解析 MCP 工具参数中的 memory_type 字符串（T7），
/// 未识别值回退 Conversation（与旧字符串透传的服务端行为一致）。
fn parse_memory_type(s: &str) -> mr_common::MemoryType {
    match s.to_lowercase().as_str() {
        "decision" => mr_common::MemoryType::Decision,
        "knowledge" => mr_common::MemoryType::Knowledge,
        "context" => mr_common::MemoryType::Context,
        "preference" => mr_common::MemoryType::Preference,
        _ => mr_common::MemoryType::Conversation,
    }
}

/// MCP 服务器，通过 stdin/stdout 与 AI 工具通信。
pub struct McpServer;

impl McpServer {
    /// 创建 MCP 服务器实例。
    pub fn new() -> Self {
        Self
    }

    /// 主循环：从 stdin 逐行读取 JSON-RPC 请求，处理后写入 stdout。
    pub async fn run(self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader
                .read_line(&mut line)
                .await
                .context("Failed to read from stdin")?;
            if n == 0 {
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let request: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    let err = json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {"code": -32700, "message": format!("Parse error: {}", e)}
                    });
                    let mut out = serde_json::to_string(&err).unwrap_or_default();
                    out.push('\n');
                    stdout.write_all(out.as_bytes()).await.ok();
                    stdout.flush().await.ok();
                    continue;
                }
            };

            let id = request.get("id").cloned();
            let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = request.get("params").cloned().unwrap_or(json!({}));

            let response = match method {
                "initialize" => self.handle_initialize(id.clone(), &params),
                "notifications/initialized" => continue,
                "tools/list" => self.handle_tools_list(id.clone()),
                "tools/call" => self.handle_tools_call(id.clone(), &params).await,
                "resources/list" => self.handle_resources_list(id.clone()),
                "resources/read" => self.handle_resources_read(id.clone(), &params).await,
                "ping" => self.make_result(id.clone(), json!({})),
                _ => self.make_error(id.clone(), -32601, &format!("Method not found: {}", method)),
            };

            let mut out = serde_json::to_string(&response).unwrap_or_default();
            out.push('\n');
            stdout
                .write_all(out.as_bytes())
                .await
                .context("Failed to write to stdout")?;
            stdout.flush().await.context("Failed to flush stdout")?;
        }

        Ok(())
    }

    /// 处理 `initialize` 方法，返回协议版本和服务器能力。
    fn handle_initialize(&self, id: Option<Value>, _params: &Value) -> Value {
        self.make_result(
            id,
            json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false }
                },
                "serverInfo": {
                    "name": "memrec",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
    }

    /// 处理 `tools/list` 方法，返回 6 个可用工具的 schema。
    fn handle_tools_list(&self, id: Option<Value>) -> Value {
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
                    "description": "List memories with optional filtering. Scope: 'project'=current project only, 'global'=global memories only, 'all'=all projects. When scope is omitted it is inferred from the .mr_pid marker in the project root (project if present, else global).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max memories to return", "default": 20 },
                            "scope": {
                                "type": "string",
                                "enum": ["project", "global", "all"],
                                "description": "List scope. Default: inferred from .mr_pid (project if present, else global)"
                            }
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
    async fn handle_tools_call(&self, id: Option<Value>, params: &Value) -> Value {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match tool_name {
            "mr_add" => self.tool_add(&arguments).await,
            "mr_search" => self.tool_search(&arguments).await,
            "mr_get" => self.tool_get(&arguments).await,
            "mr_list" => self.tool_list(&arguments).await,
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

    /// 处理 `resources/list` 方法，返回 stats 和 project 两个资源。
    fn handle_resources_list(&self, id: Option<Value>) -> Value {
        self.make_result(
            id,
            json!({
                "resources": [
                    {
                        "uri": "memrec://stats",
                        "name": "Memory Statistics",
                        "description": "Current memory statistics",
                        "mimeType": "application/json"
                    },
                    {
                        "uri": "memrec://project",
                        "name": "Project Info",
                        "description": "Current project information",
                        "mimeType": "application/json"
                    }
                ]
            }),
        )
    }

    /// 处理 `resources/read` 方法，读取指定 URI 的资源内容。
    async fn handle_resources_read(&self, id: Option<Value>, params: &Value) -> Value {
        let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");

        // T7: 资源请求转为强类型守护进程调用
        let (action, params): (RequestAction, Option<RequestParams>) = match uri {
            "memrec://stats" => (RequestAction::Stats, None),
            "memrec://project" => (
                RequestAction::GetProjectInfo,
                Some(RequestParams::GetProjectInfo(
                    GetProjectInfoParams::default(),
                )),
            ),
            _ => {
                return self.make_result(
                    id,
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "text/plain",
                            "text": format!("Unknown resource: {}", uri)
                        }]
                    }),
                )
            }
        };

        let content = match self
            .call_daemon(JsonRpcRequest::new(action, params, 1))
            .await
        {
            Ok(resp) => serde_json::to_string_pretty(&resp).unwrap_or_default(),
            Err(e) => e,
        };

        self.make_result(
            id,
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": content
                }]
            }),
        )
    }

    /// `mr_add` 工具实现：添加记忆。
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
                    working_dir: None,
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
                        .map(|mt| parse_memory_type(mt)),
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

    /// `mr_list` 工具实现：列出记忆。
    ///
    /// `scope` 三态参数与 CLI 的 `-p/-g/-a` 对齐；未指定时按 `.mr_pid` 推断。
    /// project 范围时解析 `.mr_pid` 的项目 UUID 传入，精确限定当前项目。
    async fn tool_list(&self, args: &Value) -> Result<String, String> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);
        let scope = parse_scope(args.get("scope").and_then(|v| v.as_str()));

        let (project_only, global_only, project_id) = match scope {
            Scope::Project => {
                let root = detect_working_dir().map_err(|e| e.to_string())?;
                let root = std::path::PathBuf::from(root);
                (true, false, read_project_uuid(&root))
            }
            Scope::Global => (false, true, None),
            Scope::All => (false, false, None),
        };

        let resp = self
            .call_daemon(JsonRpcRequest::new(
                RequestAction::List,
                Some(RequestParams::List(ListParams {
                    tags: vec![],
                    memory_type: None,
                    limit: limit as usize,
                    project_only,
                    global_only,
                    project_id,
                    include_deleted: false,
                    skip: 0,
                })),
                1,
            ))
            .await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
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
            return Err(format!("Dream failed: {}", msg));
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

    /// 通过 Unix Socket 调用 memrecd 守护进程。
    ///
    /// 流程：连接 → 发送请求 → 关闭写端 → 读取响应（最大 1MB）→ 解析。
    ///
    /// T7：接收强类型 [`JsonRpcRequest`]（枚举 method + 联合 params），序列化
    /// 与守护进程协议严格一致；新增接口不再需要维护"字符串方法名 + type 字段"
    /// 的双轨映射，漏配在编译期暴露。
    async fn call_daemon(&self, req: JsonRpcRequest) -> Result<Value, String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixStream;

        let home = dirs::home_dir().ok_or("Failed to get home directory")?;
        let socket_path = home.join(".memrec").join("memrecd.sock");

        let mut stream = UnixStream::connect(&socket_path)
            .await
            .map_err(|e| format!("Connect error: {}", e))?;

        let request_json =
            serde_json::to_string(&req).map_err(|e| format!("Serialize error: {}", e))?;

        stream
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        stream
            .flush()
            .await
            .map_err(|e| format!("Flush error: {}", e))?;
        stream
            .shutdown()
            .await
            .map_err(|e| format!("Shutdown error: {}", e))?;

        let mut buffer = Vec::with_capacity(8192);
        let mut chunk = vec![0u8; 8192];
        loop {
            let n = stream
                .read(&mut chunk)
                .await
                .map_err(|e| format!("Read error: {}", e))?;
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..n]);
            if buffer.len() > 1024 * 1024 {
                return Err("Response too large".to_string());
            }
        }

        let response: Value =
            serde_json::from_slice(&buffer).map_err(|e| format!("Parse error: {}", e))?;

        Ok(response)
    }

    /// 构造 JSON-RPC 成功响应。
    fn make_result(&self, id: Option<Value>, result: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        })
    }

    /// 构造 JSON-RPC 错误响应。
    fn make_error(&self, id: Option<Value>, code: i32, message: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message}
        })
    }
}

#[cfg(test)]
mod tests {
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
}
