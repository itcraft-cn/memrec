use anyhow::Result;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::signal;
use tracing::info;

use crate::storage::{RocksDBStore, MemoryStore};
use crate::server::{UnixSocketServer, Router};

pub struct Daemon {
    socket_path: PathBuf,
    data_dir: PathBuf,
}

impl Daemon {
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
        
        let data_dir = home.join(".memrec").join("data");
        let socket_path = home.join(".memrec").join("memrecd.sock");
        
        std::fs::create_dir_all(&data_dir)?;
        
        Ok(Self {
            socket_path,
            data_dir,
        })
    }
    
    pub async fn run(&self) -> Result<()> {
        info!("MemRec daemon starting");
        
        let rocksdb = RocksDBStore::open(&self.data_dir)?;
        let storage = Arc::new(MemoryStore::new(rocksdb));
        
        let router = Arc::new(Router::new(storage));
        
        let server = UnixSocketServer::bind(&self.socket_path, router).await?;
        
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())?;
        
        tokio::select! {
            _ = server.run() => {
                info!("Server stopped");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
        }
        
        self.shutdown()
    }
    
    fn shutdown(&self) -> Result<()> {
        info!("Shutting down daemon");
        
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }
        
        Ok(())
    }
}