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
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::DaemonConfig;
use crate::server::{ConnMode, Router, TestMode, UnixSocketServer};
use mr_ability::archive::ArchiveManager;
use mr_ability::dedup::{DedupDecider, RuleDedupDecider};
use mr_ability::dream::{DreamError, DreamProcessor};
use mr_ability::embedding::{EmbeddingGenerator, GeneratorFactory};
use mr_ability::idle::{DreamRequest, SystemIdleMonitor};
use mr_ability::llm::{LlmClient, OpenAiCompatibleClient};
use mr_ability::propagation::{MockDeltaGenerator, PropagationEngine};
use mr_ability::search::{MmrConfig, ScorerConfig};
use mr_ability::storage::{
    DreamRunStore, EdgeStore, FtsStorage, HybridStore, IndexingStorage, MemoryStorage, MemoryStore,
    PropagationStore, RocksDBStore, RocksDBVectorStore, TantivyStore, TieredStorage, TieredStore,
    VectorStorage,
};

/// 向量存储定时同步间隔（秒）
const SYNC_INTERVAL_SECS: u64 = 30;

/// 过期记忆清扫间隔（秒）。
///
/// 5 分钟：足够让“一次性记忆”在过期后及时退出检索池与注入预算，
/// 又不会因频繁全量扫描带来持续 IO 压力。
const EXPIRY_SWEEP_SECS: u64 = 300;

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
        let tiered_store: Arc<dyn TieredStorage> = Arc::new(TieredStore::new(rocksdb.clone()));

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

        // 启动全文索引自愈：修正历史遗留的“活跃记忆缺全文文档 / 软删记忆残留全文
        // 文档”不一致（旧版本 Dream 写路径曾绕过全文索引维护）。
        // T4 语义迁移在其前：nil→None 的记录还需要被 FTS/向量以同一判据检索。
        let migrated = storage.migrate_global_semantics().await?;
        if migrated > 0 {
            info!(
                "Global semantics migrated: {} memories (nil→None, workspace→Global)",
                migrated
            );
        }
        self.reconcile_fts_index(&storage, &fts_store).await?;

        // 启动二级索引自愈：历史记忆可能缺 by_created / by_project 索引项，
        // 补齐后按创建时间区间与项目范围的查询才能保持下推语义。
        storage.reconcile_secondary_indexes().await?;

        let hybrid_store = Arc::new(
            HybridStore::new(
                vector_store.clone(),
                fts_store,
                MmrConfig::default(),
                ScorerConfig::default(),
            )
            // T3: MMR 重排用全文而非 200 字符 preview，避免"开头相同、结论相反"
            // 的记忆被误判相似。
            .with_text_provider(
                storage.clone() as std::sync::Arc<dyn mr_ability::storage::TextProvider>
            )
            // 排序修饰（新近/重要性/来源）需要主存储中的元数据；索引载荷不携带
            // 这些字段，且历史载荷无回收路径，故按 id 回表取。
            .with_memory_storage(
                storage.clone() as std::sync::Arc<dyn mr_ability::storage::MemoryStorage>
            ),
        );

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
            let client = Arc::new(OpenAiCompatibleClient::new(self.config.llm.clone())?);
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

        // 快路径去重：规则版（同步、无 LLM）。慢路径冲突审查见下方 review 通道。
        let dedup_decider: Option<Arc<dyn DedupDecider>> =
            Some(Arc::new(RuleDedupDecider::from_config(&self.config.dedup)));

        // 检索侧指标：Router 与后台审查任务共享同一份原子计数。
        let metrics = Arc::new(crate::metrics::RetrievalMetrics::new());

        let (dream_tx, dream_rx) = mpsc::channel::<DreamRequest>(10);

        // 访问反馈：检索命中由 Router 非阻塞投递，本任务按固定间隔批量回写。
        // 通道满时丢弃（访问统计可容忍丢失），绝不阻塞检索请求。
        let (access_tx, access_rx) = mpsc::channel::<Uuid>(1024);
        let access_task = if self.config.retrieval.access_recording {
            Some(tokio::spawn(Self::access_loop(
                storage.clone(),
                access_rx,
                Duration::from_secs(self.config.retrieval.access_flush_secs.max(1)),
            )))
        } else {
            drop(access_rx);
            None
        };

        let idle_monitor =
            SystemIdleMonitor::new(self.config.idle_monitor.clone(), dream_tx.clone());

        let archive_manager = ArchiveManager::new(
            self.config.server.data_dir.clone(),
            self.config.archive.enabled,
        );

        // 测试模式：`.debug` 存在时生产 socket 拒绝写入，测试 socket 不受限。
        let test_mode = TestMode::new(
            self.config.server.test_mode_file.clone(),
            self.config.server.test_mode_max_age_secs,
        );
        if test_mode.active() {
            warn!(
                "Test mode ACTIVE ({}): writes on the production socket are rejected; use {}",
                self.config.server.test_mode_file.display(),
                self.config.server.test_socket_path.display()
            );
        }

        let router = Router::new(
                storage.clone(),
                vector_store.clone(),
                hybrid_store.clone(),
                embedder.clone(),
                dream_processor.clone(),
                archive_manager,
                scene_store,
                self.config.dedup.clone(),
                dedup_decider,
            )
            .with_dream_run_store(dream_run_store.clone())
            .with_edge_store(edge_store.clone())
            .with_tiered_store(tiered_store)
            .with_budget_policy(mr_common::TypeBudgetPolicy {
                caps: self.config.retrieval.injection_type_caps.clone(),
            })
            .with_access_tracker(access_tx)
            .with_metrics(metrics.clone())
            .with_test_mode(test_mode.clone())
            .with_retrieval_graph(crate::server::RetrievalGraph {
                enabled: self.config.dream.retrieval_graph_enabled,
                depth: self.config.dream.retrieval_graph_depth,
                decay: self.config.dream.retrieval_graph_decay,
                weight: self.config.dream.retrieval_graph_weight,
                quota: self.config.dream.retrieval_graph_quota,
                min_ratio: self.config.dream.retrieval_graph_min_ratio,
            });

        // 周期性冲突审查：仅在显式开启且 LLM 可用时启用；调度由 cron 决定。
        let review_task = if let (Some(llm), true) =
            (&llm_client, self.config.dedup.llm_review_enabled)
        {
            let reviewer = Arc::new(mr_ability::dedup::LlmConflictReviewer::new(llm.clone()));
            let state_path = self
                .config
                .server
                .data_dir
                .parent()
                .unwrap_or(&self.config.server.data_dir)
                .join("review.state");
            Some(tokio::spawn(crate::review::review_loop(
                storage.clone(),
                vector_store.clone(),
                embedder.clone(),
                hybrid_store,
                reviewer,
                metrics.clone(),
                self.config.dedup.clone(),
                state_path,
            )))
        } else {
            None
        };

        let router = Arc::new(router);
        let prod_path = self.config.server.socket_path.clone();
        let test_path = self.config.server.test_socket_path.clone();
        let prod_server =
            UnixSocketServer::bind_with_mode(&prod_path, router.clone(), ConnMode::Production)
                .await?;
        let test_server = if test_path != prod_path {
            match UnixSocketServer::bind_with_mode(&test_path, router, ConnMode::TestSocket).await {
                Ok(server) => Some(server),
                Err(e) => {
                    error!(
                        "Failed to bind test socket {}: {:#}; test mode writes will be unavailable",
                        test_path.display(),
                        e
                    );
                    None
                }
            }
        } else {
            warn!("test_socket_path equals socket_path; test socket disabled");
            None
        };

        let mut prod_task = tokio::spawn(async move {
            if let Err(e) = prod_server.run().await {
                error!("Production socket server stopped: {:#}", e);
            }
        });
        let mut test_task = test_server.map(|server| {
            tokio::spawn(async move {
                if let Err(e) = server.run().await {
                    error!("Test socket server stopped: {:#}", e);
                }
            })
        });

        let sync_task = tokio::spawn(Self::sync_loop(vector_store.clone()));
        // 过期清扫走索引一致型存储：软删同时摘除向量与全文索引，避免幽灵命中。
        let expiry_task = tokio::spawn(Self::expiry_loop(indexing_storage.clone()));
        let idle_task = tokio::spawn(idle_monitor.run());
        let dream_task = tokio::spawn(Self::dream_loop(
            dream_processor,
            dream_rx,
            test_mode.clone(),
        ));

        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())?;

        tokio::select! {
            _ = &mut prod_task => {
                info!("Server stopped");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
        }

        prod_task.abort();
        if let Some(task) = test_task.as_mut() {
            task.abort();
        }
        sync_task.abort();
        expiry_task.abort();
        idle_task.abort();
        dream_task.abort();
        if let Some(task) = access_task {
            task.abort();
        }
        if let Some(task) = review_task {
            task.abort();
        }

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
        let mut memories = storage.list(usize::MAX).await?;

        // 空内容记忆不参与向量索引：空串嵌入接近语料质心，会对任意查询稳定命中，
        // 污染排序。这里顺带摘除历史遗留的空向量（旧版本曾为其生成嵌入）。
        let mut removed = 0usize;
        for memory in memories.iter().filter(|m| m.content.trim().is_empty()) {
            if vector_store.remove(&memory.id).await.unwrap_or(false) {
                removed += 1;
            }
        }
        if removed > 0 {
            vector_store.save()?;
            info!("Removed {} vector(s) for empty-content memories", removed);
        }
        memories.retain(|m| !m.content.trim().is_empty());

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

        // 先收集缺嵌入的记忆及其向量载荷，再一次性批量嵌入：利用后端批处理能力，
        // 避免逐条 embed 在大量缺失时拖慢启动（M-3）。批量接口 `embed_batch` 已在
        // EmbeddingGenerator trait 中提供。
        let mut pending = Vec::new();
        for memory in &memories {
            if vector_store.get(&memory.id).await?.is_none() {
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
                pending.push((memory, payload));
            }
        }

        if !pending.is_empty() {
            let texts: Vec<String> = pending.iter().map(|(m, _)| m.content.clone()).collect();
            let embeddings = embedder.embed_batch(&texts)?;
            for ((memory, payload), embedding) in pending.into_iter().zip(embeddings) {
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

    /// 启动全文索引自愈：比对活跃记忆与全文索引文档数，不一致则全量重建。
    ///
    /// 旧版本 Dream 写路径绕过全文索引维护，可能残留两类不一致：活跃记忆缺全文
    /// 文档（检索不到）、软删记忆残留全文文档（“幽灵结果”）。此处以全文索引文档数
    /// 与活跃记忆数是否相等为快速判据；相等则跳过（常数代价）。不一致时以记忆库为
    /// 唯一真源**全量重建**：清空索引后按活跃记忆批量写入（仅两次提交），从而一并
    /// 清除软删残留文档、历史重复文档与悬空文档，保证文档数精确等于活跃记忆数。
    async fn reconcile_fts_index(
        &self,
        storage: &Arc<MemoryStore>,
        fts_store: &Arc<TantivyStore>,
    ) -> Result<()> {
        let all = storage.list(usize::MAX).await?;
        // 空内容记忆不入全文索引：与向量侧同一判据，避免「文档数 = 活跃记忆数」
        // 的快速判据被空文档掩盖，也避免空文档进入 BM25 结果集。
        let active: Vec<_> = all
            .into_iter()
            .filter(|m| !m.content.trim().is_empty())
            .collect();
        let fts_count = fts_store.count().await?;
        if fts_count == active.len() {
            info!("FTS index consistent: {} documents", fts_count);
            return Ok(());
        }

        info!(
            "FTS index inconsistent (docs: {}, indexable memories: {}); rebuilding...",
            fts_count,
            active.len()
        );

        // 以记忆库为唯一真源全量重建（清空 + 批量写入），避免逐条探测与提交。
        fts_store.clear().await?;
        let docs: Vec<(uuid::Uuid, String, mr_ability::storage::FtsPayload)> = active
            .iter()
            .map(|m| {
                (
                    m.id,
                    m.content.clone(),
                    mr_ability::storage::FtsPayload {
                        project_id: m.project_id,
                        memory_type: m.memory_type.to_string(),
                        tags: m.tags.clone(),
                        importance: m.importance,
                    },
                )
            })
            .collect();
        fts_store.add_many(&docs).await?;
        fts_store.reload()?;

        info!(
            "FTS index rebuilt: {} documents (was {})",
            fts_store.count().await?,
            fts_count
        );
        Ok(())
    }

    /// 访问反馈批量回写循环。
    ///
    /// 接收检索命中的记忆 id，去重后按 `flush` 间隔（或攒满 512 条）一次性写入。
    /// 单次失败只告警并丢弃本批，避免坏记录长期占用内存或无限重试。
    async fn access_loop(
        storage: Arc<MemoryStore>,
        mut receiver: mpsc::Receiver<Uuid>,
        flush: Duration,
    ) {
        let mut pending: HashSet<Uuid> = HashSet::new();
        let mut ticker = interval(flush);
        ticker.tick().await;

        loop {
            tokio::select! {
                maybe = receiver.recv() => match maybe {
                    Some(id) => {
                        pending.insert(id);
                        if pending.len() >= 512 {
                            Self::flush_access(&storage, &mut pending).await;
                        }
                    }
                    None => break,
                },
                _ = ticker.tick() => {
                    Self::flush_access(&storage, &mut pending).await;
                }
            }
        }

        Self::flush_access(&storage, &mut pending).await;
    }

    /// 冲刷一批访问统计。
    async fn flush_access(storage: &MemoryStore, pending: &mut HashSet<Uuid>) {
        if pending.is_empty() {
            return;
        }
        let ids: Vec<Uuid> = pending.drain().collect();
        match storage.touch_access(&ids).await {
            Ok(updated) => {
                tracing::debug!(target: "recall", "access flushed: {} memories", updated);
            }
            Err(e) => {
                tracing::warn!(target: "recall", "access flush failed: {:#}", e);
            }
        }
    }

    /// 向量存储定时同步循环，每 [`SYNC_INTERVAL_SECS`] 秒将缓存刷盘。
    ///
    /// 仅在存在脏数据时实际写入；无论是否有变更均以 debug 记录，
    /// 默认 INFO 级下静默，避免高频同步日志淹没其他信息。
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
                        tracing::debug!(
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

    /// 过期记忆清扫循环。
    ///
    /// 启动后立即执行一轮（清理停机期间到期的记忆），此后每
    /// [`EXPIRY_SWEEP_SECS`] 秒一轮。清扫失败只告警，不影响守护进程存活。
    async fn expiry_loop(storage: Arc<dyn MemoryStorage>) {
        let mut ticker = interval(Duration::from_secs(EXPIRY_SWEEP_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            match mr_ability::expiry::sweep_expired(storage.as_ref(), chrono::Utc::now()).await {
                Ok(report) if report.swept > 0 => info!(
                    "Expiry sweep: {} of {} scanned memories soft-deleted",
                    report.swept, report.scanned
                ),
                Ok(_) => {}
                Err(e) => warn!("Expiry sweep failed: {:#}", e),
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
        test_mode: TestMode,
    ) {
        info!("Dream loop started");

        while let Some(request) = receiver.recv().await {
            // 测试期间暂停 Dream：避免后台整合改写数据、干扰测试断言。
            if test_mode.active() {
                tracing::debug!(target: "dream", "Dream skipped: test mode active");
                continue;
            }
            // 触发通告降为 debug：真正有信息量的是本次执行的结果（完成/跳过/失败），
            // 避免空闲期每 5 分钟一条重复的"已触发"。
            tracing::debug!(target: "dream", "Dream triggered: force={}", request.force);

            match processor.execute(request.force).await {
                Ok(result) => {
                    info!(
                        target: "dream",
                        "Dream completed: integrated {} memories",
                        result.integrated_count
                    );
                }
                // 门控正常拦截（距上次过近/新增不足等）属预期跳过，用 debug 记录，
                // 不作为告警，避免淹没真实故障。
                Err(DreamError::GateSkipped { message, .. }) => {
                    tracing::debug!(target: "dream", "Dream skipped: {}", message);
                }
                Err(e) => {
                    warn!(target: "dream", "Dream failed: {}", e);
                }
            }
        }

        info!("Dream loop stopped");
    }
}
