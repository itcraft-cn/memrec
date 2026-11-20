//! # Unix 域套接字服务器
//!
//! 监听 Unix Socket 接受客户端连接，每个连接独立 tokio 任务处理。
//! 通信协议为 JSON-RPC 2.0，单次请求-响应模式。
//!
//! ## 分帧与限额
//!
//! `SOCK_STREAM` 不保证一次 `read` 读完整个请求：内核可能按需分片，也可能
//! 单次返回少于发送量。因此读取必须**累计到能完整解析为止**，而不是一次
//! `read` 就当完整请求处理（旧实现固定 8192 字节单次读取，超过该长度的
//! 请求后半段被丢弃，导致 CLI 之外的客户端——MCP、DSH 插件——写入大正文必失败）。
//!
//! 请求上限 `MAX_REQUEST_BYTES`、读超时 `READ_TIMEOUT`、并发连接上限
//! `MAX_CONCURRENT_CONNECTIONS` 三者为硬约束：超限请求回 `-32600`，
//! 慢连接在超时后被关闭，连接数超限时直接拒绝新连接，避免 fd 被耗尽。

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

use super::handler::Router;
use super::test_mode::ConnMode;
use mr_protocol::{JsonRpcError, JsonRpcResponse};

/// 单个请求的最大字节数（与客户端响应上限 1MB 对齐）。
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// 读取一个完整请求的超时时间。
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// 写回响应的超时时间（大响应体的客户端可能较慢）。
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);

/// 同时处理的连接上限，超出后直接关闭新连接，避免 fd 耗尽。
const MAX_CONCURRENT_CONNECTIONS: usize = 128;

/// 每次 `read` 的缓冲区大小（与请求上限解耦，循环累计）。
const READ_CHUNK: usize = 16 * 1024;

/// Unix 域套接字服务器。
///
/// 绑定到指定路径的 Unix Socket，接受连接后读取请求、
/// 路由处理、写回响应，然后关闭连接。
pub struct UnixSocketServer {
    listener: UnixListener,
    router: Arc<Router>,
    /// 连接来源：生产 socket 或测试 socket
    mode: ConnMode,
}

impl UnixSocketServer {
    /// 绑定到指定 Socket 路径。
    ///
    /// 若路径已存在则先删除旧文件，然后创建新的监听器。
    pub async fn bind(socket_path: &Path, router: Arc<Router>) -> Result<Self> {
        Self::bind_with_mode(socket_path, router, ConnMode::Production).await
    }

    /// 绑定指定路径并按连接来源处理请求。
    pub async fn bind_with_mode(
        socket_path: &Path,
        router: Arc<Router>,
        mode: ConnMode,
    ) -> Result<Self> {
        if socket_path.exists() {
            std::fs::remove_file(socket_path).context("Failed to remove existing socket file")?;
        }

        let listener = UnixListener::bind(socket_path).context("Failed to bind Unix socket")?;

        info!(
            "Unix socket bound at {} (mode={:?})",
            socket_path.display(),
            mode
        );

        Ok(Self {
            listener,
            router,
            mode,
        })
    }

    /// 运行服务器主循环，持续接受连接。
    ///
    /// 每个连接在独立的 tokio 任务中处理，不会阻塞后续连接；
    /// 并发连接数受 `MAX_CONCURRENT_CONNECTIONS` 约束，超限的新连接
    /// 会被立即关闭并记录告警，避免慢连接把 fd 耗尽。
    pub async fn run(&self) -> Result<()> {
        info!("Unix socket server started");

        let slots = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));

        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let router = self.router.clone();
                    let mode = self.mode;
                    let permit = match slots.clone().try_acquire_owned() {
                        Ok(permit) => permit,
                        Err(_) => {
                            warn!("连接数已达上限 {}，拒绝新连接", MAX_CONCURRENT_CONNECTIONS);
                            drop(stream);
                            continue;
                        }
                    };
                    tokio::spawn(async move {
                        // permit 随任务结束释放，确保并发额度与任务生命周期一致
                        let _permit = permit;
                        if let Err(e) = Self::handle_connection(stream, router, mode).await {
                            warn!("Connection handling failed: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// 处理单个连接：累计读取完整请求 → 路由 → 写回响应 → 关闭。
    ///
    /// 读取采用「循环累计 + 每次尝试解析」策略：`serde_json` 只有在输入完整时
    /// 才会成功，因此只要能解析出请求即可立即进入路由，无需依赖分隔符或
    /// 猜测长度。超时、超限与读错误都返回 `Err`，由调用方统一记录告警。
    async fn handle_connection(
        mut stream: tokio::net::UnixStream,
        router: Arc<Router>,
        mode: ConnMode,
    ) -> Result<()> {
        let request =
            match tokio::time::timeout(READ_TIMEOUT, Self::read_request(&mut stream)).await {
                // 读取阶段超时：慢连接不能无限占用任务与 fd
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "读取请求超时（{:?}），已关闭连接",
                        READ_TIMEOUT
                    ))
                }
                Ok(result) => result?,
            };

        let response = match router.parse_request(&request) {
            Ok(req) => {
                let resp = router.route_with_mode(req, mode).await;
                router.serialize_response(&resp)?
            }
            // 解析失败必须回合法 JSON-RPC 错误（含 jsonrpc/id/整数错误码），
            // 否则客户端拿到的只是 {"error":"..."}，无法反序列化也无从关联请求。
            Err(e) => router.serialize_response(&JsonRpcResponse::error(
                JsonRpcError {
                    code: -32700,
                    message: format!("Parse error: {}", e),
                    data: None,
                },
                0,
            ))?,
        };

        tokio::time::timeout(WRITE_TIMEOUT, async {
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;
            stream.shutdown().await
        })
        .await
        .map_err(|_| anyhow::anyhow!("写回响应超时（{:?}）", WRITE_TIMEOUT))?
        .context("Failed to write response")?;

        Ok(())
    }

    /// 累计读取一个完整请求。
    ///
    /// 返回的字符串要么能被 `serde_json` 解析为对象，要么是触发上限前读到的
    /// 全部内容（交由调用方报解析错误）。超过 `MAX_REQUEST_BYTES` 时返回错误，
    /// 由调用方翻译成 `-32600`/`-32700`。
    async fn read_request(stream: &mut tokio::net::UnixStream) -> Result<String> {
        let mut buffer: Vec<u8> = Vec::with_capacity(READ_CHUNK);
        let mut chunk = vec![0u8; READ_CHUNK];

        loop {
            let n = stream
                .read(&mut chunk)
                .await
                .context("Failed to read from stream")?;

            if n == 0 {
                // 对端关闭写端：若已读到内容则视作请求结束，否则视为空连接
                if buffer.is_empty() {
                    return Err(anyhow::anyhow!("Connection closed without a request"));
                }
                break;
            }

            buffer.extend_from_slice(&chunk[..n]);

            if buffer.len() > MAX_REQUEST_BYTES {
                return Err(anyhow::anyhow!("请求超过上限 {} 字节", MAX_REQUEST_BYTES));
            }

            // 完整的 JSON 对象才可能被解析成功；成功即停止读取
            if serde_json::from_slice::<serde_json::Value>(&buffer).is_ok() {
                break;
            }
        }

        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_ability::archive::ArchiveManager;
    use mr_ability::dream::DreamProcessor;
    use mr_ability::embedding::{EmbeddingGenerator, MockEmbedder};
    use mr_ability::search::{MmrConfig, ScorerConfig};
    use mr_ability::storage::{
        HybridStore, MemoryStore, RocksDBStore, SceneStore, TantivyStore, VectorStore,
    };
    use mr_common::DreamConfig;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_unix_socket_bind() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(std::sync::Arc::new(rocksdb)));
        let embedder = Arc::new(MockEmbedder::new());
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(TantivyStore::new_test());
        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));
        let dream_config = DreamConfig::default();
        let data_dir = std::path::PathBuf::from("/tmp/memrec_test");
        let dream_processor = Arc::new(DreamProcessor::new(
            dream_config,
            &data_dir,
            storage.clone(),
        ));
        let archive_manager = ArchiveManager::new(data_dir.clone(), true);

        // scene 存储使用独立临时目录，避免与 storage 的 RocksDB 锁冲突
        let scene_dir = tempdir().unwrap();
        let scene_path = scene_dir.path().to_path_buf();
        std::mem::forget(scene_dir);
        let rocksdb_for_scene = Arc::new(RocksDBStore::open(&scene_path).unwrap());
        let scene_store = Arc::new(SceneStore::new(rocksdb_for_scene));

        let router = Arc::new(Router::new(
            storage,
            vector_store,
            hybrid_store,
            embedder,
            dream_processor,
            archive_manager,
            scene_store,
            mr_common::DedupConfig::default(),
            None,
        ));

        let _server = UnixSocketServer::bind(&socket_path, router).await.unwrap();
        assert!(socket_path.exists());
    }

    /// 超过 8192 字节且分片到达的请求必须被完整读取。
    ///
    /// 回归背景：旧实现固定单次 `read` 8192 字节，超过该长度的请求后半段被丢弃，
    /// 只有 CLI（按 7500 字节分块）能规避，MCP 与 DSH 插件写入大正文必失败。
    #[tokio::test]
    async fn test_read_request_reads_fragmented_large_payload() {
        let (mut client, mut server) = tokio::net::UnixStream::pair().unwrap();

        // 约 40KB 正文，远超旧实现的 8192 字节单次读取上限
        let body = "x".repeat(40 * 1024);
        let request = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"add\",\"id\":1,\"params\":{{\"type\":\"add\",\"content\":\"{}\"}}}}",
            body
        );
        let expected_len = request.len();

        let writer = tokio::spawn(async move {
            // 故意分片写，模拟内核按到达量返回而非一次读完
            for chunk in request.as_bytes().chunks(4096) {
                client.write_all(chunk).await.unwrap();
                client.flush().await.unwrap();
            }
            client.shutdown().await.unwrap();
        });

        let read = UnixSocketServer::read_request(&mut server).await.unwrap();
        writer.await.unwrap();

        assert_eq!(read.len(), expected_len, "应读取完整请求而非截断到 8192");
        assert!(
            serde_json::from_str::<serde_json::Value>(&read).is_ok(),
            "累计读取的内容必须是完整 JSON"
        );
    }

    /// 超过 `MAX_REQUEST_BYTES` 的请求必须返回错误，不得无界占用内存。
    #[tokio::test]
    async fn test_read_request_rejects_oversize() {
        let (mut client, mut server) = tokio::net::UnixStream::pair().unwrap();

        let writer = tokio::spawn(async move {
            let filler = vec![b'x'; 64 * 1024];
            for _ in 0..(MAX_REQUEST_BYTES / filler.len() + 2) {
                if client.write_all(&filler).await.is_err() {
                    break;
                }
            }
            let _ = client.shutdown().await;
        });

        let result = UnixSocketServer::read_request(&mut server).await;
        // 读取端已提前返回错误，写入端可能阻塞在写满的 socket 缓冲上；
        // 直接取消而不等待，避免测试自身死锁。
        writer.abort();

        assert!(result.is_err(), "超限请求应返回错误");
        assert!(
            result.unwrap_err().to_string().contains("上限"),
            "错误信息应说明超过上限"
        );
    }
}
