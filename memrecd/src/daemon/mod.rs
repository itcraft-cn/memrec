//! # 守护进程主循环
//!
//! [`Daemon`] 是 memrecd 的核心结构，负责：
//!
//! 1. 加载配置并校验模型就绪状态
//! 2. 初始化 RocksDB 存储、向量存储、全文索引和嵌入生成器
//! 3. 启动 Unix Socket 服务器接收 JSON-RPC 请求
//! 4. 运行定时同步任务（向量存储持久化）
//! 5. 监听 SIGTERM/SIGINT 信号优雅关闭

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::time::interval;
use tracing::{info, warn};

use crate::config::DaemonConfig;
use crate::embedding::{EmbeddingGenerator, GeneratorFactory};
use crate::search::{MmrConfig, ScorerConfig};
use crate::server::{Router, UnixSocketServer};
use crate::storage::{
    HybridStore, MemoryStorage, MemoryStore, RocksDBStore, RocksDBVectorStore,
    TantivyStore, VectorStorage,
};

/// 向量存储定时同步间隔（秒）
const SYNC_INTERVAL_SECS: u64 = 30;

/// 守护进程主结构，持有配置并协调各子系统生命周期。
pub struct Daemon {
    config: DaemonConfig,
}

impl Daemon {
    /// 从默认配置路径加载配置创建守护进程。
    pub fn new() -> Result<Self> {
        let config = DaemonConfig::load()?;
        Ok(Self { config })
    }

    /// 使用指定配置创建守护进程。
    pub fn with_config(config: DaemonConfig) -> Self {
        Self { config }
    }

    /// 从命令行参数创建守护进程，支持覆盖模型类型和模型目录。
    pub fn from_args(
        model_type: Option<memrec_common::ModelType>,
        model_dir: Option<String>,
    ) -> Result<Self> {
        let mut config = DaemonConfig::load()?;

        if let Some(mt) = model_type {
            config = config.with_model(mt);
        }

        if let Some(dir) = model_dir {
            config = config.with_model_dir(dir);
        }

        Ok(Self { config })
    }

    /// 运行守护进程主循环。
    ///
    /// 流程：校验模型 → 打开存储 → 重建缺失嵌入 → 启动服务器 → 等待信号 → 关闭清理
    pub async fn run(&self) -> Result<()> {
        info!(
            "MemRec daemon starting with model: {}",
            self.config.model.model_type.name()
        );

        info!("Data dir: {:?}", self.config.server.data_dir);
        info!("Vectors dir: {:?}", self.config.server.vectors_dir);
        info!("Socket: {:?}", self.config.server.socket_path);

        if !self.config.model.is_ready() {
            anyhow::bail!(
                "Model configuration is not ready. Please run mr-install to download the model."
            );
        }

        let rocksdb = RocksDBStore::open(&self.config.server.data_dir)?;
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let embedder = GeneratorFactory::create(self.config.model.clone())?;
        let vector_store = Arc::new(RocksDBVectorStore::open(
            &self.config.server.vectors_dir,
            embedder.dimension(),
        )?);

        let fts_dir = self.config.server.data_dir.parent()
            .unwrap_or(&self.config.server.data_dir)
            .join("fts");
        let fts_store = Arc::new(TantivyStore::open(&fts_dir).await?);

        let hybrid_store = Arc::new(HybridStore::new(
            vector_store.clone(),
            fts_store,
            MmrConfig::default(),
            ScorerConfig::default(),
        ));

        self.rebuild_missing_embeddings(&storage, &vector_store, &embedder)
            .await?;

        let router = Arc::new(Router::new(
            storage.clone(),
            vector_store.clone(),
            hybrid_store,
            embedder,
        ));

        let server = UnixSocketServer::bind(&self.config.server.socket_path, router).await?;

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

    /// 重建缺失的嵌入向量。
    ///
    /// 启动时对比记忆数量与向量存储缓存数量，
    /// 为缺失嵌入的记忆重新生成向量并持久化。
    async fn rebuild_missing_embeddings(
        &self,
        storage: &Arc<MemoryStore>,
        vector_store: &Arc<RocksDBVectorStore>,
        embedder: &Arc<dyn EmbeddingGenerator>,
    ) -> Result<()> {
        let memories = storage.list(1000).await?;
        let existing_count = vector_store.count_cached();

        if existing_count >= memories.len() {
            info!("All {} memories have embeddings", memories.len());
            return Ok(());
        }

        info!(
            "Rebuilding embeddings for {} memories (existing: {})",
            memories.len() - existing_count,
            existing_count
        );

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
        info!(
            "Rebuild complete, saved {} embeddings",
            vector_store.count_cached()
        );

        Ok(())
    }

    /// 向量存储定时同步循环，每 [`SYNC_INTERVAL_SECS`] 秒将缓存刷盘。
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

    /// 优雅关闭：保存向量存储、清理 Unix Socket 文件。
    fn shutdown(&self, vector_store: &Arc<RocksDBVectorStore>) -> Result<()> {
        info!("Shutting down daemon");

        if let Err(e) = vector_store.save() {
            warn!("Failed to save vector store on shutdown: {}", e);
        } else {
            info!("Vector store saved on shutdown");
        }

        if self.config.server.socket_path.exists() {
            std::fs::remove_file(&self.config.server.socket_path)?;
        }

        Ok(())
    }
}
