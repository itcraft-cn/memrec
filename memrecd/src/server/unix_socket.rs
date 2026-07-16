//! # Unix 域套接字服务器
//!
//! 监听 Unix Socket 接受客户端连接，每个连接独立 tokio 任务处理。
//! 通信协议为 JSON-RPC 2.0，单次请求-响应模式。

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tracing::{debug, error, info};

use super::handler::Router;

/// Unix 域套接字服务器。
///
/// 绑定到指定路径的 Unix Socket，接受连接后读取请求、
/// 路由处理、写回响应，然后关闭连接。
pub struct UnixSocketServer {
    listener: UnixListener,
    router: Arc<Router>,
}

impl UnixSocketServer {
    /// 绑定到指定 Socket 路径。
    ///
    /// 若路径已存在则先删除旧文件，然后创建新的监听器。
    pub async fn bind(socket_path: &Path, router: Arc<Router>) -> Result<Self> {
        if socket_path.exists() {
            std::fs::remove_file(socket_path).context("Failed to remove existing socket file")?;
        }

        let listener = UnixListener::bind(socket_path).context("Failed to bind Unix socket")?;

        info!("Unix socket bound at {}", socket_path.display());

        Ok(Self { listener, router })
    }

    /// 运行服务器主循环，持续接受连接。
    ///
    /// 每个连接在独立的 tokio 任务中处理，不会阻塞后续连接。
    pub async fn run(&self) -> Result<()> {
        info!("Unix socket server started");

        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    debug!("Accepted new connection");
                    let router = self.router.clone();
                    tokio::spawn(async move {
                        Self::handle_connection(stream, router).await;
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// 处理单个连接：读取请求 → 路由 → 写回响应 → 关闭。
    async fn handle_connection(mut stream: tokio::net::UnixStream, router: Arc<Router>) {
        let mut buffer = vec![0u8; 8192];

        match stream.read(&mut buffer).await {
            Ok(0) => {
                debug!("Connection closed");
            }
            Ok(n) => {
                let request = String::from_utf8_lossy(&buffer[..n]);
                debug!("Received request: {}", request);

                let response = match router.parse_request(&request) {
                    Ok(req) => {
                        let resp = router.route(req).await;
                        router
                            .serialize_response(&resp)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
                    }
                    Err(e) => format!("{{\"error\": \"{}\"}}", e),
                };

                if let Err(e) = stream.write_all(response.as_bytes()).await {
                    error!("Failed to write response: {}", e);
                    return;
                }

                if let Err(e) = stream.flush().await {
                    error!("Failed to flush stream: {}", e);
                    return;
                }

                if let Err(e) = stream.shutdown().await {
                    error!("Failed to shutdown stream: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to read from stream: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::{EmbeddingGenerator, FastEmbedGenerator};
    use crate::search::{MmrConfig, ScorerConfig};
    use crate::storage::{HybridStore, MemoryStore, RocksDBStore, TantivyStore, VectorStore};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_unix_socket_bind() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));
        let model_config = memrec_common::ModelConfig::default();
        let embedder = Arc::new(FastEmbedGenerator::new(model_config).unwrap());
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let fts_store = Arc::new(TantivyStore::new_test());
        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));
        let router = Arc::new(Router::new(storage, vector_store, hybrid_store, embedder));

        let server = UnixSocketServer::bind(&socket_path, router).await.unwrap();
        assert!(socket_path.exists());
    }
}
