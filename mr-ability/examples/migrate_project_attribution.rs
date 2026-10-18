//! 一次性迁移工具：把误归属到单一项目桶的历史记忆按标签/别名回迁到正确项目。
//!
//! 临时工具，非常驻 API；执行前须停止 memrecd（以独占方式打开 RocksDB）。
//! 后续项目归属由写入路径（`mr_add` 传递调用方工作目录）自动带入，本工具仅用于
//! 清理历史上因写入路由缺陷而汇入同一桶的记忆。
//!
//! 用法：
//! ```text
//! cargo run --release --example migrate_project_attribution -- \
//!   --data-dir ~/.memrec/data \
//!   --vectors-dir ~/.memrec/vectors \
//!   --fts-dir ~/.memrec/fts \
//!   --mapping migration.json \
//!   [--archive-db ~/.memrec/data/archive/raw_2026.db] \
//!   [--dimension 1024] \
//!   [--report report.json] \
//!   [--apply]
//! ```
//!
//! 缺省为 dry-run，只读取与统计，不写库；加 `--apply` 才实际迁移。
//!
//! `mapping.json` 结构：
//! ```json
//! {
//!   "source_project": "b435a636-481b-43dd-a819-cc2cedebf365",
//!   "projects": [
//!     { "name": "QES3", "uuid": "2c0569ee-...", "aliases": ["qes3", "cltspothk"] }
//!   ]
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use mr_ability::archive::SqliteArchive;
use mr_ability::embedding::MockEmbedder;
use mr_ability::search::{MmrConfig, ScorerConfig};
use mr_ability::storage::{
    HybridStorage, HybridStore, IndexingStorage, MemoryStorage, MemoryStore, RocksDBStore,
    RocksDBVectorStore, TantivyStore, VectorStorage,
};
use mr_common::Memory;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 单个目标项目规则：项目名 + UUID + 可识别该项目的标签别名。
#[derive(Debug, Deserialize)]
struct ProjectRule {
    name: String,
    uuid: Uuid,
    #[serde(default)]
    aliases: Vec<String>,
}

/// 迁移映射文件。
#[derive(Debug, Deserialize)]
struct PathMapping {
    /// 误归属的源项目桶（其下记忆将被回迁）。
    source_project: Uuid,
    projects: Vec<ProjectRule>,
}

impl PathMapping {
    /// 构建 `别名(小写) -> 目标项目 UUID` 索引；项目名本身亦作为别名。
    fn tag_index(&self) -> HashMap<String, Uuid> {
        let mut idx = HashMap::new();
        for p in &self.projects {
            idx.insert(p.name.to_lowercase(), p.uuid);
            for a in &p.aliases {
                idx.insert(a.to_lowercase(), p.uuid);
            }
        }
        idx
    }

    /// 依据记忆标签解析目标项目：取标签列表中首个命中别名的项。
    fn resolve(&self, memory: &Memory, idx: &HashMap<String, Uuid>) -> Option<Uuid> {
        memory
            .tags
            .iter()
            .find_map(|tag| idx.get(&tag.to_lowercase()).copied())
    }
}

/// 未决记忆的摘要，便于人工复核。
#[derive(Debug, Serialize)]
struct UnresolvedSample {
    id: Uuid,
    tags: Vec<String>,
    preview: String,
}

/// 迁移报告。
#[derive(Debug, Default, Serialize)]
struct MigrationReport {
    applied: bool,
    source_project: Uuid,
    scanned_active: usize,
    scanned_deleted: usize,
    migrated: usize,
    unresolved: usize,
    missing_embedding: usize,
    per_target: HashMap<String, usize>,
    unresolved_samples: Vec<UnresolvedSample>,
}

/// 命令行参数。
struct Args {
    data_dir: String,
    vectors_dir: String,
    fts_dir: String,
    mapping: String,
    archive_db: Option<String>,
    dimension: usize,
    report: Option<String>,
    apply: bool,
}

impl Args {
    fn parse() -> anyhow::Result<Self> {
        let mut data_dir = None;
        let mut vectors_dir = None;
        let mut fts_dir = None;
        let mut mapping = None;
        let mut archive_db = None;
        let mut dimension = 1024usize;
        let mut report = None;
        let mut apply = false;

        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < argv.len() {
            match argv[i].as_str() {
                "--data-dir" => {
                    data_dir = Some(argv.get(i + 1).cloned().unwrap_or_default());
                    i += 2;
                }
                "--vectors-dir" => {
                    vectors_dir = Some(argv.get(i + 1).cloned().unwrap_or_default());
                    i += 2;
                }
                "--fts-dir" => {
                    fts_dir = Some(argv.get(i + 1).cloned().unwrap_or_default());
                    i += 2;
                }
                "--mapping" => {
                    mapping = Some(argv.get(i + 1).cloned().unwrap_or_default());
                    i += 2;
                }
                "--archive-db" => {
                    archive_db = Some(argv.get(i + 1).cloned().unwrap_or_default());
                    i += 2;
                }
                "--dimension" => {
                    dimension = argv.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1024);
                    i += 2;
                }
                "--report" => {
                    report = Some(argv.get(i + 1).cloned().unwrap_or_default());
                    i += 2;
                }
                "--apply" => {
                    apply = true;
                    i += 1;
                }
                other => anyhow::bail!("unknown argument: {}", other),
            }
        }

        Ok(Self {
            data_dir: data_dir.ok_or_else(|| anyhow::anyhow!("--data-dir is required"))?,
            vectors_dir: vectors_dir.ok_or_else(|| anyhow::anyhow!("--vectors-dir is required"))?,
            fts_dir: fts_dir.ok_or_else(|| anyhow::anyhow!("--fts-dir is required"))?,
            mapping: mapping.ok_or_else(|| anyhow::anyhow!("--mapping is required"))?,
            archive_db,
            dimension,
            report,
            apply,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse()?;

    let mapping: PathMapping = serde_json::from_str(&std::fs::read_to_string(&args.mapping)?)?;
    let tag_index = mapping.tag_index();

    // 独占打开主存储与索引；内存向量存储与 mock 嵌入器仅用于复用既有向量重建索引。
    let rocksdb = Arc::new(RocksDBStore::open(Path::new(&args.data_dir))?);
    let memory_store: Arc<dyn MemoryStorage> = Arc::new(MemoryStore::new(rocksdb.clone()));

    // 关键：RocksDBVectorStore 的 add 仅更新内存缓存并置 dirty，remove 却直接
    // 删除 RocksDB；若不显式 save 刷盘，被 remove 的向量将永久丢失。
    let vector_store_impl = Arc::new(RocksDBVectorStore::open(
        Path::new(&args.vectors_dir),
        args.dimension,
    )?);
    let vector_store: Arc<dyn VectorStorage> = vector_store_impl.clone();
    let fts_store = Arc::new(TantivyStore::open(Path::new(&args.fts_dir)).await?);
    let hybrid: Arc<dyn HybridStorage> = Arc::new(HybridStore::new(
        vector_store.clone(),
        fts_store,
        MmrConfig::default(),
        ScorerConfig::default(),
    ));
    let embedder = Arc::new(MockEmbedder::new());
    let indexing: Arc<dyn MemoryStorage> =
        Arc::new(IndexingStorage::new(memory_store.clone(), hybrid, embedder));

    let archive = match &args.archive_db {
        Some(path) => Some(SqliteArchive::open(Path::new(path))?),
        None => None,
    };

    let source = mapping.source_project;
    let active = memory_store.list_by_project(&source).await?;
    let deleted: Vec<Memory> = memory_store
        .list_deleted()
        .await?
        .into_iter()
        .filter(|m| m.project_id == Some(source))
        .collect();

    let mut report = MigrationReport {
        applied: args.apply,
        source_project: source,
        scanned_active: active.len(),
        scanned_deleted: deleted.len(),
        ..Default::default()
    };

    // 软删记忆不入检索索引，仅用底层存储回迁归属；活跃记忆经 IndexingStorage 同步索引。
    for m in active.iter().chain(deleted.iter()) {
        match mapping.resolve(m, &tag_index) {
            Some(target) if target != source => {
                let mut migrated = m.clone();
                let embedding = vector_store.get(&m.id).await.ok().flatten();
                let has_embedding = embedding.is_some();
                if !has_embedding {
                    report.missing_embedding += 1;
                }
                migrated.embedding = embedding;
                migrated.project_id = Some(target);

                if args.apply {
                    // 缺向量时避免走重建索引：reindex 会先摘除旧 FTS 文档，若其后
                    // 无法重写向量将丢失全文索引。此类记忆仅用底层存储回迁归属，
                    // FTS 载荷由守护进程启动时的对账重建归一。
                    if m.is_deleted || !has_embedding {
                        memory_store.save(&migrated).await?;
                    } else {
                        indexing.save(&migrated).await?;
                    }
                    if let Some(archive) = &archive {
                        archive.archive_sync(&migrated)?;
                    }
                }

                report.migrated += 1;
                let key = mapping
                    .projects
                    .iter()
                    .find(|p| p.uuid == target)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| target.to_string());
                *report.per_target.entry(key).or_insert(0) += 1;
            }
            _ => {
                report.unresolved += 1;
                if report.unresolved_samples.len() < 40 {
                    report.unresolved_samples.push(UnresolvedSample {
                        id: m.id,
                        tags: m.tags.clone(),
                        preview: m.content.chars().take(80).collect(),
                    });
                }
            }
        }
    }

    // 落盘向量缓存：add 只改内存，必须显式 save 才能持久化迁移后的向量与载荷。
    if args.apply {
        vector_store_impl.save()?;
    }

    // 摘要输出。
    println!(
        "[migrate] mode={} source={} active={} deleted={} migrated={} unresolved={} missing_embedding={}",
        if args.apply { "APPLY" } else { "DRY-RUN" },
        source,
        report.scanned_active,
        report.scanned_deleted,
        report.migrated,
        report.unresolved,
        report.missing_embedding
    );
    let mut per_target: Vec<_> = report.per_target.iter().collect();
    per_target.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in per_target {
        println!("  -> {:<24} {}", name, count);
    }
    if !report.unresolved_samples.is_empty() {
        println!("[migrate] unresolved 样例（标签 / 正文片段）：");
        for s in &report.unresolved_samples {
            println!("  {:?} | {}", s.tags, s.preview);
        }
    }

    if let Some(path) = &args.report {
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        println!("[migrate] report written: {}", path);
    }

    Ok(())
}
