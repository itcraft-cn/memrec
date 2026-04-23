use anyhow::Result;
use std::sync::Arc;
use std::path::PathBuf;
use std::time::Duration;
use tokio::signal;
use tokio::time::interval;
use tracing::{info, warn};

use crate::storage::{RocksDBStore, MemoryStore, RocksDBVectorStore, MemoryStorage, VectorStorage};
use crate::embedding::FastEmbedGenerator;
use crate::server::{UnixSocketServer, Router};

const SYNC_INTERVAL_SECS: u64 = 30;

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
        
        let embedder = Arc::new(FastEmbedGenerator::new()?);
        let vector_store = Arc::new(RocksDBVectorStore::open(&self.data_dir.join("vectors"), embedder.dimension())?);
        
        self.rebuild_missing_embeddings(&storage, &vector_store, &embedder).await?;
        
        let router = Arc::new(Router::new(storage.clone(), vector_store.clone(), embedder));
        
        let server = UnixSocketServer::bind(&self.socket_path, router).await?;
        
        let sync_task = tokio::spawn(Self::sync_loop(vector_store.clone()));
        
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
        
        sync_task.abort();
        
        self.shutdown(&vector_store)
    }
    
    async fn rebuild_missing_embeddings(
        &self,
        storage: &Arc<MemoryStore>,
        vector_store: &Arc<RocksDBVectorStore>,
        embedder: &Arc<FastEmbedGenerator>,
    ) -> Result<()> {
        let memories = storage.list(1000).await?;
        let existing_count = vector_store.count_cached();
        
        if existing_count >= memories.len() {
            info!("All {} memories have embeddings", memories.len());
            return Ok(());
        }
        
        info!("Rebuilding embeddings for {} memories (existing: {})", 
            memories.len() - existing_count, existing_count);
        
        for memory in &memories {
            if vector_store.get(&memory.id).await?.is_none() {
                let embedding = embedder.embed(&memory.content)?;
                let payload = crate::storage::VectorPayload {
                    project_id: memory.project_id,
                    memory_type: memory.memory_type.to_string(),
                    tags: memory.tags.clone(),
                    content_preview: memory.content.chars().take(200).collect(),
                    importance: memory.importance,
                    chunk_group_id: memory.chunk_group_id,
                    chunk_index: memory.chunk_index,
                    chunk_total: memory.chunk_total,
                };
                vector_store.add(&memory.id, &embedding, payload).await?;
            }
        }
        
        vector_store.save()?;
        info!("Rebuild complete, saved {} embeddings", vector_store.count_cached());
        
        Ok(())
    }
    
    async fn sync_loop(vector_store: Arc<RocksDBVectorStore>) {
        let mut ticker = interval(Duration::from_secs(SYNC_INTERVAL_SECS));
        
        loop {
            ticker.tick().await;
            
            if let Err(e) = vector_store.save() {
                warn!("Failed to sync vector store: {}", e);
            } else {
                info!("Vector store synced");
            }
        }
    }
    
    fn shutdown(&self, vector_store: &Arc<RocksDBVectorStore>) -> Result<()> {
        info!("Shutting down daemon");
        
        if let Err(e) = vector_store.save() {
            warn!("Failed to save vector store on shutdown: {}", e);
        } else {
            info!("Vector store saved on shutdown");
        }
        
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }
        
        Ok(())
    }
}