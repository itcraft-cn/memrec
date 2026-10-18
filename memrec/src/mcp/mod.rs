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

/// 计算写入路径（`mr_add`）应携带的工作目录。
///
/// 全局记忆不携带工作目录（归属由 `is_global` 决定为全局）；
/// 非全局记忆取调用方的项目根/当前目录，供守护进程据此解析 `.mr_pid`。
/// 与 `tool_search` 的项目范围处理一致，保证两个入口归属语义对称。
fn add_working_dir(is_global: bool) -> Option<String> {
    if is_global {
        None
    } else {
        detect_working_dir().ok()
    }
}

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
            // 响应上限与 CLI 客户端一致（64MB）：列表类响应可随记忆数量增长，
            // 上限仅用于兜底防御异常巨大响应。
            if buffer.len() > 64 * 1024 * 1024 {
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

mod tests;
mod tools;
