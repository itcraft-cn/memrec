use anyhow::{Result, Context};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct McpServer;

impl McpServer {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await.context("Failed to read from stdin")?;
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
            stdout.write_all(out.as_bytes()).await.context("Failed to write to stdout")?;
            stdout.flush().await.context("Failed to flush stdout")?;
        }

        Ok(())
    }

    fn handle_initialize(&self, id: Option<Value>, _params: &Value) -> Value {
        self.make_result(id, json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false }
            },
            "serverInfo": {
                "name": "memrec",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))
    }

    fn handle_tools_list(&self, id: Option<Value>) -> Value {
        self.make_result(id, json!({
            "tools": [
                {
                    "name": "memory_add",
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
                    "name": "memory_search",
                    "description": "Search memories using semantic similarity. Returns results scored by relevance (0-1).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search query" },
                            "top_k": { "type": "integer", "description": "Max results to return", "default": 10 },
                            "min_score": { "type": "number", "description": "Minimum similarity score (0-1)", "default": 0.75 },
                            "project_only": { "type": "boolean", "description": "Search only current project" },
                            "global_only": { "type": "boolean", "description": "Search only global memories" },
                            "cross_project": { "type": "boolean", "description": "Search across all projects" },
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
                    "name": "memory_get",
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
                    "name": "memory_list",
                    "description": "List memories with optional filtering.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max memories to return", "default": 20 },
                            "project_only": { "type": "boolean", "description": "List only current project" },
                            "global_only": { "type": "boolean", "description": "List only global memories" }
                        }
                    }
                },
                {
                    "name": "memory_delete",
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
                    "name": "memory_stats",
                    "description": "Get memory statistics.",
                    "inputSchema": { "type": "object", "properties": {} }
                }
            ]
        }))
    }

    async fn handle_tools_call(&self, id: Option<Value>, params: &Value) -> Value {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match tool_name {
            "memory_add" => self.tool_add(&arguments).await,
            "memory_search" => self.tool_search(&arguments).await,
            "memory_get" => self.tool_get(&arguments).await,
            "memory_list" => self.tool_list(&arguments).await,
            "memory_delete" => self.tool_delete(&arguments).await,
            "memory_stats" => self.tool_stats().await,
            _ => Err(format!("Unknown tool: {}", tool_name)),
        };

        match result {
            Ok(content) => self.make_result(id, json!({
                "content": [{"type": "text", "text": content}],
                "isError": false
            })),
            Err(msg) => self.make_result(id, json!({
                "content": [{"type": "text", "text": msg}],
                "isError": true
            })),
        }
    }

    fn handle_resources_list(&self, id: Option<Value>) -> Value {
        self.make_result(id, json!({
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
        }))
    }

    async fn handle_resources_read(&self, id: Option<Value>, params: &Value) -> Value {
        let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");

        let content = match uri {
            "memrec://stats" => {
                match self.call_daemon("stats", json!({})).await {
                    Ok(resp) => serde_json::to_string_pretty(&resp).unwrap_or_default(),
                    Err(e) => e,
                }
            }
            "memrec://project" => {
                match self.call_daemon("get_project_info", json!({})).await {
                    Ok(resp) => serde_json::to_string_pretty(&resp).unwrap_or_default(),
                    Err(e) => e,
                }
            }
            _ => format!("Unknown resource: {}", uri),
        };

        self.make_result(id, json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": content
            }]
        }))
    }

    async fn tool_add(&self, args: &Value) -> Result<String, String> {
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let memory_type = args.get("memory_type").and_then(|v| v.as_str()).unwrap_or("conversation");
        let tags: Vec<String> = args.get("tags")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let is_global = args.get("is_global").and_then(|v| v.as_bool()).unwrap_or(false);

        let params = json!({
            "content": content,
            "memory_type": memory_type,
            "tags": tags,
            "is_global": is_global
        });

        let resp = self.call_daemon("add", params).await?;

        let memory_id = resp.get("result")
            .and_then(|r| r.get("memory"))
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Ok(format!("Added memory: {} (type: {}, global: {})", memory_id, memory_type, is_global))
    }

    async fn tool_search(&self, args: &Value) -> Result<String, String> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let top_k = args.get("top_k").and_then(|v| v.as_u64()).unwrap_or(10);
        let min_score = args.get("min_score").and_then(|v| v.as_f64()).unwrap_or(0.75);
        let project_only = args.get("project_only").and_then(|v| v.as_bool()).unwrap_or(false);
        let global_only = args.get("global_only").and_then(|v| v.as_bool()).unwrap_or(false);
        let cross_project = args.get("cross_project").and_then(|v| v.as_bool()).unwrap_or(false);
        let memory_type = args.get("memory_type").and_then(|v| v.as_str());

        let mut params = json!({
            "query": query,
            "top_k": top_k,
            "min_score": min_score,
            "project_only": project_only,
            "global_only": global_only,
            "cross_project": cross_project,
            "include_global": !project_only
        });

        if let Some(mt) = memory_type {
            params["memory_type"] = json!(mt);
        }

        let resp = self.call_daemon("search_memory", params).await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    async fn tool_get(&self, args: &Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let merge = args.get("merge").and_then(|v| v.as_bool()).unwrap_or(false);

        let params = json!({ "id": id, "merge": merge });
        let resp = self.call_daemon("get", params).await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    async fn tool_list(&self, args: &Value) -> Result<String, String> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);
        let project_only = args.get("project_only").and_then(|v| v.as_bool()).unwrap_or(false);
        let global_only = args.get("global_only").and_then(|v| v.as_bool()).unwrap_or(false);

        let params = json!({ "limit": limit, "project_only": project_only, "global_only": global_only });
        let resp = self.call_daemon("list", params).await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    async fn tool_delete(&self, args: &Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");

        let params = json!({ "id": id });
        let resp = self.call_daemon("delete", params).await?;

        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    async fn tool_stats(&self) -> Result<String, String> {
        let resp = self.call_daemon("stats", json!({})).await?;
        Ok(serde_json::to_string_pretty(&resp).unwrap_or_default())
    }

    async fn call_daemon(&self, method: &str, mut params: Value) -> Result<Value, String> {
        use tokio::net::UnixStream;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let home = dirs::home_dir().ok_or("Failed to get home directory")?;
        let socket_path = home.join(".memrec").join("memrecd.sock");

        let mut stream = UnixStream::connect(&socket_path).await
            .map_err(|e| format!("Connect error: {}", e))?;

        let (method_str, typed_params) = match method {
            "add" => ("add", {
                if !params.is_object() { params = json!({}); }
                params["type"] = json!("add");
                Some(params)
            }),
            "get" => ("get", {
                if !params.is_object() { params = json!({}); }
                params["type"] = json!("get");
                Some(params)
            }),
            "delete" => ("delete", {
                if !params.is_object() { params = json!({}); }
                params["type"] = json!("delete");
                Some(params)
            }),
            "list" => ("list", {
                if !params.is_object() { params = json!({}); }
                params["type"] = json!("list");
                Some(params)
            }),
            "search_memory" => ("search_memory", {
                if !params.is_object() { params = json!({}); }
                params["type"] = json!("search_memory");
                Some(params)
            }),
            "get_project_info" => ("get_project_info", {
                if !params.is_object() { params = json!({}); }
                params["type"] = json!("get_project_info");
                Some(params)
            }),
            "stats" => ("stats", None),
            "get_version" => ("get_version", None),
            _ => return Err(format!("Unknown method: {}", method)),
        };

        let request = json!({
            "jsonrpc": "2.0",
            "method": method_str,
            "params": typed_params,
            "id": 1
        });

        let request_json = serde_json::to_string(&request)
            .map_err(|e| format!("Serialize error: {}", e))?;

        stream.write_all(request_json.as_bytes()).await
            .map_err(|e| format!("Write error: {}", e))?;
        stream.flush().await.map_err(|e| format!("Flush error: {}", e))?;
        stream.shutdown().await.map_err(|e| format!("Shutdown error: {}", e))?;

        let mut buffer = Vec::with_capacity(8192);
        let mut chunk = vec![0u8; 8192];
        loop {
            let n = stream.read(&mut chunk).await.map_err(|e| format!("Read error: {}", e))?;
            if n == 0 { break; }
            buffer.extend_from_slice(&chunk[..n]);
            if buffer.len() > 1024 * 1024 {
                return Err("Response too large".to_string());
            }
        }

        let response: Value = serde_json::from_slice(&buffer)
            .map_err(|e| format!("Parse error: {}", e))?;

        Ok(response)
    }

    fn make_result(&self, id: Option<Value>, result: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        })
    }

    fn make_error(&self, id: Option<Value>, code: i32, message: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message}
        })
    }
}
