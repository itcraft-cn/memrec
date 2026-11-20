//! # Unix Socket 客户端连接
//!
//! [`Client`] 通过 Unix Socket 与 memrecd 守护进程通信，
//! 发送 JSON-RPC 2.0 请求并接收响应。

use anyhow::{Context, Result};
use mr_protocol::{JsonRpcRequest, JsonRpcResponse};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// 初始缓冲区大小
const INITIAL_BUFFER_SIZE: usize = 8192;
/// 最大响应大小。
///
/// 列表类响应会随记忆数量与正文长度增长（实测 `list -a -l 400` 约 3.7MB），
/// 上限过小会误伤合法的批量查询。此处放宽到 64MB，仍保留上界以防异常巨大响应耗尽内存。
const MAX_BUFFER_SIZE: usize = 64 * 1024 * 1024;

/// memrecd 客户端，通过 Unix Socket 发送 JSON-RPC 请求。
pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    /// 创建客户端，使用默认 Socket 路径 `~/.memrec/memrecd.sock`。
    pub fn new() -> Result<Self> {
        let socket_path = Self::default_socket_path()?;
        Ok(Self { socket_path })
    }

    /// 默认 Socket 路径：`~/.memrec/memrecd.sock`
    ///
    /// 可通过 `MEMREC_SOCKET_PATH` 覆盖（测试实例、隔离环境）。
    fn default_socket_path() -> Result<PathBuf> {
        if let Ok(path) = std::env::var("MEMREC_SOCKET_PATH") {
            if !path.trim().is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(home.join(".memrec").join("memrecd.sock"))
    }

    /// 发送 JSON-RPC 请求并接收响应。
    ///
    /// 流程：连接 → 发送请求 → 关闭写端 → 读取响应 → 解析。
    /// 响应超过 1MB 时返回错误。
    pub async fn send(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        // 连接与整体读响应都必须有超时：守护进程卡死时客户端不能无界挂起
        // （调用方常是 Agent，挂起会连带阻塞整轮对话）。
        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            UnixStream::connect(&self.socket_path),
        )
        .await
        .map_err(|_| anyhow::anyhow!("连接 memrecd 超时（30s）：{}", self.socket_path.display()))?
        .with_context(|| {
            format!(
                "Failed to connect to memrecd: {}",
                self.socket_path.display()
            )
        })?;

        let request_json = serde_json::to_string(request).context("Failed to serialize request")?;

        stream
            .write_all(request_json.as_bytes())
            .await
            .context("Failed to send request")?;

        stream.flush().await.context("Failed to flush stream")?;

        stream
            .shutdown()
            .await
            .context("Failed to shutdown stream")?;

        let mut buffer = Vec::with_capacity(INITIAL_BUFFER_SIZE);
        let mut chunk = vec![0u8; INITIAL_BUFFER_SIZE];

        loop {
            let n = stream
                .read(&mut chunk)
                .await
                .context("Failed to read response")?;

            if n == 0 {
                break;
            }

            buffer.extend_from_slice(&chunk[..n]);

            if buffer.len() >= MAX_BUFFER_SIZE {
                return Err(anyhow::anyhow!(
                    "Response too large (exceeds {}MB)",
                    MAX_BUFFER_SIZE / (1024 * 1024)
                ));
            }
        }

        let response_json = String::from_utf8_lossy(&buffer);
        let response: JsonRpcResponse =
            serde_json::from_str(&response_json).context("Failed to parse response")?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_default_socket_path() {
        let path = Client::default_socket_path().unwrap();
        assert!(path.to_string_lossy().contains(".memrec"));
    }
}
