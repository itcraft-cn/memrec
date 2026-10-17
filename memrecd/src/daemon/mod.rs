//! # 守护进程主循环
//!
//! [`Daemon`] 是 memrecd 的核心结构，负责：
//!
//! 1. 加载配置并校验模型就绪状态
//! 2. 初始化 RocksDB 存储、向量存储、全文索引和嵌入生成器
//! 3. 启动 Unix Socket 服务器接收 JSON-RPC 请求
//! 4. 运行定时同步任务（向量存储持久化）
//! 5. 运行空闲监控和 Dream 整合任务
//! 6. 监听 SIGTERM/SIGINT 信号优雅关闭

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::config::DaemonConfig;
use crate::server::{Router, UnixSocketServer};
use mr_ability::archive::ArchiveManager;
use mr_ability::dedup::{DedupDecider, RuleDedupDecider};
use mr_ability::dream::DreamProcessor;
use mr_ability::embedding::{EmbeddingGenerator, GeneratorFactory};
use mr_ability::idle::{DreamRequest, SystemIdleMonitor};
use mr_ability::llm::{LlmClient, OpenAiCompatibleClient};
use mr_ability::propagation::{MockDeltaGenerator, PropagationEngine};
use mr_ability::search::{MmrConfig, ScorerConfig};
use mr_ability::storage::{
    DreamRunStore, EdgeStore, HybridStore, IndexingStorage, MemoryStorage, MemoryStore,
    PropagationStore, RocksDBStore, RocksDBVectorStore, TantivyStore, VectorStorage,
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
        model_type: Option<mr_common::ModelType>,
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
            error!(
                "Model configuration is not ready: no file manifest. Run `mr-install` to download the model."
            );
            anyhow::bail!(
                "Model configuration is not ready. Please run mr-install to download the model."
            );
        }

        // 启动预检：必需模型文件缺失时立即显式报错，同时写入日志文件与 stderr，
        // 避免仅在深层初始化（FastEmbed 加载）中失败而"静默退出"。
        let model_dir =
            mr_ability::embedding::resolve_model_dir(&self.config.model).map_err(|e| {
                error!("Failed to resolve embedding model directory: {}", e);
                e
            })?;
        let missing = mr_ability::embedding::missing_required_files(&self.config.model)?;
        if !missing.is_empty() {
            error!(
                "Embedding model files missing in {}: {}. Run `mr-install --skip-install --skip-service --skip-verify --use-hf-mirror` to download.",
                model_dir.display(),
                missing.join(", ")
            );
            anyhow::bail!(
                "Embedding model files missing in {}: {}",
                model_dir.display(),
                missing.join(", ")
            );
        }
        info!("Model files verified in {}", model_dir.display());

        let rocksdb = Arc::new(RocksDBStore::open(&self.config.server.data_dir)?);
        let storage = Arc::new(MemoryStore::new(rocksdb.clone()));
        let scene_store = Arc::new(mr_ability::storage::SceneStore::new(rocksdb.clone()));
        // 图机制存储：边、传播增量与 DreamRun 审计（与记忆共享同一 DB 实例）
        let edge_store: Arc<dyn mr_ability::storage::EdgeStorage> =
            Arc::new(EdgeStore::new(rocksdb.clone()));
        let propagation_store: Arc<dyn mr_ability::storage::PropagationStorage> =
            Arc::new(PropagationStore::new(rocksdb.clone()));
        let dream_run_store: Arc<dyn mr_ability::storage::DreamRunStorage> =
            Arc::new(DreamRunStore::new(rocksdb.clone()));

        let embedder = GeneratorFactory::create(self.config.model.clone())?;
        let vector_store = Arc::new(RocksDBVectorStore::open(
            &self.config.server.vectors_dir,
            embedder.dimension(),
        )?);

        let fts_dir = self
            .config
            .server
            .data_dir
            .parent()
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

        // 索引一致型存储：Dream 链路的全部写操作（融合产出、软删源、阶段清理/摘要）
        // 统一经此装饰器同步到混合检索索引，避免产出不可检索、软删源仍被返回。
        let hybrid_index: Arc<dyn mr_ability::storage::HybridStorage> = hybrid_store.clone();
        let indexing_storage: Arc<dyn MemoryStorage> = Arc::new(IndexingStorage::new(
            storage.clone(),
            hybrid_index,
            embedder.clone(),
        ));

        // LLM 装配：未启用时强制 Dream 视为不启用
        let llm_client: Option<Arc<dyn LlmClient>> = if self.config.llm.enabled {
            let client = Arc::new(OpenAiCompatibleClient::new(self.config.llm.clone()));
            info!(
                "LLM enabled: provider={}, model={}, think_level={}",
                self.config.llm.provider, self.config.llm.model, self.config.llm.think_level
            );
            Some(client)
        } else {
            warn!(
                "LLM not configured ([llm].enabled=false); Dream integration will be treated as disabled"
            );
            None
        };

        // 依赖 LLM 的能力（Dream 摘要）需 LLM 就绪；否则按用户要求视为 Dream 不启用
        let mut dream_cfg = self.config.dream.clone();
        if llm_client.is_none() {
            dream_cfg.enabled = false;
        }
        let mut dream_builder = DreamProcessor::new(
            dream_cfg,
            &self.config.server.data_dir,
            indexing_storage.clone(),
        )
        .with_vector_store(vector_store.clone())
        .with_embedder(embedder.clone())
        .with_edge_store(edge_store.clone())
        .with_run_store(dream_run_store.clone())
        .with_propagation(Arc::new(PropagationEngine::new(
            mr_common::PropagationConfig::default(),
            storage.clone(),
            edge_store.clone(),
            propagation_store.clone(),
            Arc::new(MockDeltaGenerator::default()),
        )));
        if let Some(llm) = &llm_client {
            dream_builder = dream_builder.with_llm(llm.clone());
        }
        let dream_processor = Arc::new(dream_builder);

        // 去重决策器：规则版（LLM 版预留演进）
        let dedup_decider: Option<Arc<dyn DedupDecider>> =
            Some(Arc::new(RuleDedupDecider::from_config(&self.config.dedup)));

        let (dream_tx, dream_rx) = mpsc::channel::<DreamRequest>(10);

        let idle_monitor =
            SystemIdleMonitor::new(self.config.idle_monitor.clone(), dream_tx.clone());

        let archive_manager = ArchiveManager::new(
            self.config.server.data_dir.clone(),
            self.config.archive.enabled,
        );

        let router = Arc::new(
            Router::new(
                storage.clone(),
                vector_store.clone(),
                hybrid_store,
                embedder,
                dream_processor.clone(),
                archive_manager,
                scene_store,
                self.config.dedup.clone(),
                dedup_decider,
            )
            .with_dream_run_store(dream_run_store.clone())
            .with_edge_store(edge_store.clone())
            .with_retrieval_graph(crate::server::RetrievalGraph {
                enabled: self.config.dream.retrieval_graph_enabled,
                depth: self.config.dream.retrieval_graph_depth,
                decay: self.config.dream.retrieval_graph_decay,
                weight: self.config.dream.retrieval_graph_weight,
            }),
        );

        let server = UnixSocketServer::bind(&self.config.server.socket_path, router).await?;

        let sync_task = tokio::spawn(Self::sync_loop(vector_store.clone()));
        let idle_task = tokio::spawn(idle_monitor.run());
        let dream_task = tokio::spawn(Self::dream_loop(dream_processor, dream_rx));

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
        idle_task.abort();
        dream_task.abort();

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
        let memories = storage.list(usize::MAX).await?;
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
                let payload = mr_ability::storage::VectorPayload {
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
    ///
    /// 仅在存在脏数据时实际写入并记录 INFO（携带条数与耗时）；
    /// 无变更时静默（trace），避免每 30 秒一条无意义日志。
    async fn sync_loop(vector_store: Arc<RocksDBVectorStore>) {
        let mut ticker = interval(Duration::from_secs(SYNC_INTERVAL_SECS));

        loop {
            ticker.tick().await;

            let dirty = vector_store.is_dirty();
            let count = vector_store.count_cached();
            let start = std::time::Instant::now();

            match vector_store.save() {
                Ok(()) => {
                    if dirty {
                        info!(
                            "Vector store synced: {} vectors, took {:?}",
                            count,
                            start.elapsed()
                        );
                    } else {
                        tracing::trace!("Vector store sync skipped (no changes)");
                    }
                }
                Err(e) => warn!("Failed to sync vector store: {}", e),
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

    /// Dream 整合循环，接收空闲监控触发请求并执行 Dream。
    async fn dream_loop(
        processor: Arc<DreamProcessor>,
        mut receiver: mpsc::Receiver<DreamRequest>,
    ) {
        info!("Dream loop started");

        while let Some(request) = receiver.recv().await {
            info!(target: "dream", "Dream triggered: force={}", request.force);

            match processor.execute(request.force).await {
                Ok(result) => {
                    info!(
                        target: "dream",
                        "Dream completed: integrated {} memories",
                        result.integrated_count
                    );
                }
                Err(e) => {
                    warn!(target: "dream", "Dream failed: {}", e);
                }
            }
        }

        info!("Dream loop stopped");
    }
}
