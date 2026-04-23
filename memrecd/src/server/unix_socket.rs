use anyhow::{Result, Context};
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::path::Path;
use std::sync::Arc;
use tracing::{info, error, debug};

use super::handler::Router;

pub struct UnixSocketServer {
    listener: UnixListener,
    router: Arc<Router>,
}

impl UnixSocketServer {
    pub async fn bind(socket_path: &Path, router: Arc<Router>) -> Result<Self> {
        if socket_path.exists() {
            std::fs::remove_file(socket_path)
                .context("Failed to remove existing socket file")?;
        }
        
        let listener = UnixListener::bind(socket_path)
            .context("Failed to bind Unix socket")?;
        
        info!("Unix socket bound at {}", socket_path.display());
        
        Ok(Self { listener, router })
    }
    
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
                        router.serialize_response(&resp).unwrap_or_else(|e| {
                            format!("{{\"error\": \"{}\"}}", e)
                        })
                    }
                    Err(e) => format!("{{\"error\": \"{}\"}}", e)
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
    use crate::storage::{MemoryStore, RocksDBStore, VectorStore};
    use crate::embedding::FastEmbedGenerator;
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_unix_socket_bind() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));
        let embedder = Arc::new(FastEmbedGenerator::new().unwrap());
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));
        let router = Arc::new(Router::new(storage, vector_store, embedder));
        
        let server = UnixSocketServer::bind(&socket_path, router).await.unwrap();
        assert!(socket_path.exists());
    }
}