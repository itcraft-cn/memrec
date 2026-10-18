//! 一次性项目归属迁移工具（WS2 配套，临时，非常驻 API）。
//!
//! 两种模式：
//! 1. 标签映射（默认）：把误归属到单一项目桶的历史记忆，按标签/别名回迁到正确项目。
//! 2. 显式指派：`--assign <uuid> --ids <逗号分隔 id>` 把指定记忆直接归属到目标项目，
//!    可选 `--restore-deleted` 同时恢复其中的软删除记忆。
//!
//! 执行前须停止 memrecd（以独占方式打开 RocksDB）。后续项目归属由写入路径
//! （`mr_add` 传递调用方工作目录）自动带入，本工具仅用于清理历史误归属。
//!
//! 用法：
//! ```text
//! # 标签映射
//! cargo run --release --example migrate_project_attribution -- \
//!   --data-dir ~/.memrec/data --vectors-dir ~/.memrec/vectors --fts-dir ~/.memrec/fts \
//!   --mapping scripts/project_migration/mapping.json \
//!   [--archive-db ~/.memrec/data/archive/raw_2026.db] [--dimension 1024] [--apply]
//!
//! # 显式指派（可恢复软删）
//! cargo run --release --example migrate_project_attribution -- \
//!   --data-dir ... --vectors-dir ... --fts-dir ... \
//!   --assign <目标项目 uuid> --ids <id1,id2,...> --restore-deleted [--apply]
//! ```
//!
//! 缺省为 dry-run，只读取与统计，不写库；加 `--apply` 才实际迁移。

use std::collections::{HashMap, HashSet};
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

    /// 目标项目可读名（无匹配则回退 UUID 字符串）。
    fn name_of(&self, uuid: Uuid) -> String {
        self.projects
            .iter()
            .find(|p| p.uuid == uuid)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| uuid.to_string())
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
    restored: usize,
    unresolved: usize,
    missing_embedding: usize,
    /// 显式 `--ids` 中在存储里找不到的 id。
    missing_ids: Vec<Uuid>,
    per_target: HashMap<String, usize>,
    unresolved_samples: Vec<UnresolvedSample>,
}

/// 命令行参数。
struct Args {
    data_dir: String,
    vectors_dir: String,
    fts_dir: String,
    mapping: Option<String>,
    assign_to: Option<Uuid>,
    ids: Vec<Uuid>,
    restore_deleted: bool,
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
        let mut assign_to = None;
        let mut ids: Vec<Uuid> = Vec::new();
        let mut restore_deleted = false;
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
                "--assign" => {
                    let raw = argv.get(i + 1).cloned().unwrap_or_default();
                    assign_to =
                        Some(Uuid::parse_str(&raw).map_err(|_| {
                            anyhow::anyhow!("--assign 需要合法 UUID，收到: {}", raw)
                        })?);
                    i += 2;
                }
                "--ids" => {
                    let raw = argv.get(i + 1).cloned().unwrap_or_default();
                    for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        ids.push(
                            Uuid::parse_str(token)
                                .map_err(|_| anyhow::anyhow!("--ids 含非法 UUID: {}", token))?,
                        );
                    }
                    i += 2;
                }
                "--restore-deleted" => {
                    restore_deleted = true;
                    i += 1;
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

        if mapping.is_none() && (assign_to.is_none() || ids.is_empty()) {
            anyhow::bail!("需要 --mapping，或 --assign <uuid> 搭配 --ids <id,...>");
        }

        Ok(Self {
            data_dir: data_dir.ok_or_else(|| anyhow::anyhow!("--data-dir is required"))?,
            vectors_dir: vectors_dir.ok_or_else(|| anyhow::anyhow!("--vectors-dir is required"))?,
            fts_dir: fts_dir.ok_or_else(|| anyhow::anyhow!("--fts-dir is required"))?,
            mapping,
            assign_to,
            ids,
            restore_deleted,
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

    let mapping: Option<PathMapping> = match &args.mapping {
        Some(path) => Some(serde_json::from_str(&std::fs::read_to_string(path)?)?),
        None => None,
    };
    let tag_index = mapping.as_ref().map(|m| m.tag_index()).unwrap_or_default();

    // 显式指派：id -> 目标项目。
    let explicit: HashMap<Uuid, Uuid> = match args.assign_to {
        Some(target) => args.ids.iter().map(|id| (*id, target)).collect(),
        None => HashMap::new(),
    };

    // 独占打开主存储与索引；mock 嵌入器仅用于复用既有向量重建索引。
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

    // 收集候选：源桶（标签模式）+ 显式 id（不在候选集者按 id 直接加载）。
    let mut candidates: Vec<Memory> = Vec::new();
    let mut report = MigrationReport {
        applied: args.apply,
        source_project: mapping
            .as_ref()
            .map(|m| m.source_project)
            .unwrap_or(Uuid::nil()),
        ..Default::default()
    };

    if let Some(mapping) = &mapping {
        let active = memory_store
            .list_by_project(&mapping.source_project)
            .await?;
        let deleted: Vec<Memory> = memory_store
            .list_deleted()
            .await?
            .into_iter()
            .filter(|m| m.project_id == Some(mapping.source_project))
            .collect();
        report.scanned_active = active.len();
        report.scanned_deleted = deleted.len();
        candidates.extend(active);
        candidates.extend(deleted);
    }

    let seen: HashSet<Uuid> = candidates.iter().map(|m| m.id).collect();
    for id in &args.ids {
        if seen.contains(id) {
            continue;
        }
        match memory_store.get(id).await? {
            Some(m) => candidates.push(m),
            None => report.missing_ids.push(*id),
        }
    }

    // 处理候选：目标 = 显式指派优先，否则标签映射。
    for m in &candidates {
        let target = explicit
            .get(&m.id)
            .copied()
            .or_else(|| mapping.as_ref().and_then(|mp| mp.resolve(m, &tag_index)));
        let Some(target) = target else {
            report.unresolved += 1;
            if report.unresolved_samples.len() < 40 {
                report.unresolved_samples.push(UnresolvedSample {
                    id: m.id,
                    tags: m.tags.clone(),
                    preview: m.content.chars().take(80).collect(),
                });
            }
            continue;
        };

        let embedding = vector_store.get(&m.id).await.ok().flatten();
        let has_embedding = embedding.is_some();
        if !has_embedding {
            report.missing_embedding += 1;
        }

        // 软删记忆：显式指派且要求恢复时，先 restore 清软删索引再改归属；
        // 否则仅用底层存储改归属（保持软删，不入检索索引）。
        let restore = args.restore_deleted && m.is_deleted && explicit.contains_key(&m.id);

        let mut migrated = m.clone();
        migrated.embedding = embedding;
        migrated.project_id = Some(target);
        if restore {
            migrated.is_deleted = false;
            migrated.deleted_at = None;
            report.restored += 1;
        }

        if args.apply {
            if restore {
                // restore 清软删索引并使 is_deleted=false 落盘；随后改归属重建索引。
                memory_store.restore(&m.id).await?;
            }
            // 缺向量时避免走重建索引：reindex 会先摘除旧 FTS 文档，若其后无法
            // 重写向量将丢失全文索引。此类记忆仅用底层存储回迁归属。
            if migrated.is_deleted || !has_embedding {
                memory_store.save(&migrated).await?;
            } else {
                indexing.save(&migrated).await?;
            }
            if let Some(archive) = &archive {
                archive.archive_sync(&migrated)?;
            }
        }

        report.migrated += 1;
        let key = match &mapping {
            Some(mp) => mp.name_of(target),
            None => target.to_string(),
        };
        *report.per_target.entry(key).or_insert(0) += 1;
    }

    // 落盘向量缓存：add 只改内存，必须显式 save 才能持久化迁移后的向量与载荷。
    if args.apply {
        vector_store_impl.save()?;
    }

    // 摘要输出。
    println!(
        "[migrate] mode={} migrated={} restored={} unresolved={} missing_embedding={} missing_ids={}",
        if args.apply { "APPLY" } else { "DRY-RUN" },
        report.migrated,
        report.restored,
        report.unresolved,
        report.missing_embedding,
        report.missing_ids.len()
    );
    let mut per_target: Vec<_> = report.per_target.iter().collect();
    per_target.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in per_target {
        println!("  -> {:<28} {}", name, count);
    }
    if !report.unresolved_samples.is_empty() {
        println!("[migrate] unresolved 样例（标签 / 正文片段）：");
        for s in &report.unresolved_samples {
            println!("  {:?} | {}", s.tags, s.preview);
        }
    }
    for id in &report.missing_ids {
        println!("[migrate] missing id: {}", id);
    }

    if let Some(path) = &args.report {
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        println!("[migrate] report written: {}", path);
    }

    Ok(())
}
