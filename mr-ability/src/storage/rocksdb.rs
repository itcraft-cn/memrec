//! # RocksDB 底层封装
//!
//! 对 RocksDB 的列族（Column Family）操作进行封装，
//! 为上层 [`MemoryStore`]、[`FacetStore`]、[`EdgeStore`] 和 [`RocksDBVectorStore`] 提供基础读写能力。
//!
//! ## 列族划分
//!
//! | 列族 | 用途 |
//! |------|------|
//! | `memories` | 记忆主存储（UUID → JSON） |
//! | `by_tag` | 标签索引（`tag:id` → UUID） |
//! | `deleted` | 已删除记忆索引 |
//! | `importance` | 重要性索引（UUID → f64 字符串） |
//! | `projects` | 项目信息 |
//! | `config` | 配置键值对 |
//! | `facets` | Facet 主存储（UUID → JSON） |
//! | `memory_facets` | 记忆→Facet 索引（`memory_id:facet_id` → UUID） |
//! | `edges` | 边主存储（UUID → JSON） |
//! | `node_edges_out` | 出边索引（`source_id:edge_type` → `[edge_ids]`） |
//! | `node_edges_in` | 入边索引（`target_id:edge_type` → `[edge_ids]`） |
//! | `propagation_deltas` | 传播增量主存储（UUID → JSON） |
//! | `pending_propagations` | 待应用增量队列（`target_id:priority:delta_id` → UUID） |
//! | `applied_propagations` | 已应用增量索引（`source_id:target_id` → UUID） |
//! | `rules` | 检索规则主存储（UUID → JSON） |
//! | `rule_stats` | 规则统计（UUID → JSON） |
//! | `memory_summaries` | 记忆摘要（UUID → String） |
//! | `memory_keywords` | 记忆关键词（UUID → JSON） |
//! | `scenes` | 场景主存储（UUID → JSON） |
//! | `by_scene` | 场景-记忆索引（`scene_id:memory_id` → UUID） |
//! | `by_heat` | 热度索引（`heat:scene_id` → UUID） |
//! | `by_theme` | 主题索引（`theme:scene_id` → UUID） |
//! | `dream_runs` | DreamRun 审计记录（run_id → JSON） |

use anyhow::{Context, Result};
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use std::path::Path;

/// 记忆主存储列族
const CF_MEMORIES: &str = "memories";
/// 标签索引列族
const CF_BY_TAG: &str = "by_tag";
/// 已删除记忆索引列族
const CF_DELETED: &str = "deleted";
/// 重要性索引列族
const CF_IMPORTANCE: &str = "importance";
/// 创建时间二级索引列族（key: unix 秒大端 8B | 记忆 id 16B，value 空）。
/// 使得按创建时间区间筛选无需全量扫描（Dream 候选、新记忆计数）。
const CF_BY_CREATED: &str = "by_created";
/// 项目二级索引列族（key: 项目 id 16B，None/nil 项目统一落 16B 全零桶，value: 记忆 id 16B）。
/// None 与 `Some(Uuid::nil())` 同桶，保证两种全局语义一致可查。
const CF_BY_PROJECT: &str = "by_project";
/// 项目信息列族
const CF_PROJECTS: &str = "projects";
/// 配置键值对列族
const CF_CONFIG: &str = "config";
/// Facet 主存储列族
const CF_FACETS: &str = "facets";
/// 记忆→Facet 索引列族
const CF_MEMORY_FACETS: &str = "memory_facets";
/// 边主存储列族
const CF_EDGES: &str = "edges";
/// 出边索引列族
const CF_NODE_EDGES_OUT: &str = "node_edges_out";
/// 入边索引列族
const CF_NODE_EDGES_IN: &str = "node_edges_in";
/// 传播增量主存储列族
const CF_PROPAGATION_DELTAS: &str = "propagation_deltas";
/// 待应用增量队列列族
const CF_PENDING_PROPAGATIONS: &str = "pending_propagations";
/// 已应用增量索引列族
const CF_APPLIED_PROPAGATIONS: &str = "applied_propagations";
/// 检索规则主存储列族
const CF_RULES: &str = "rules";
/// 规则统计列族
const CF_RULE_STATS: &str = "rule_stats";
/// 记忆摘要列族
const CF_MEMORY_SUMMARIES: &str = "memory_summaries";
/// 记忆关键词列族
const CF_MEMORY_KEYWORDS: &str = "memory_keywords";
/// 场景主存储列族
const CF_SCENES: &str = "scenes";
/// 场景-记忆索引列族
const CF_BY_SCENE: &str = "by_scene";
/// 热度索引列族
const CF_BY_HEAT: &str = "by_heat";
/// 主题索引列族
const CF_BY_THEME: &str = "by_theme";
/// DreamRun 审计列族
const CF_DREAM_RUNS: &str = "dream_runs";

/// RocksDB 存储封装，提供列族访问和基础读写操作。
pub struct RocksDBStore {
    db: DB,
}

impl RocksDBStore {
    /// 打开 RocksDB 数据库，自动创建所有列族。
    pub fn open(path: &Path) -> Result<Self> {
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_MEMORIES, Options::default()),
            ColumnFamilyDescriptor::new(CF_BY_TAG, Options::default()),
            ColumnFamilyDescriptor::new(CF_DELETED, Options::default()),
            ColumnFamilyDescriptor::new(CF_IMPORTANCE, Options::default()),
            ColumnFamilyDescriptor::new(CF_BY_CREATED, Options::default()),
            ColumnFamilyDescriptor::new(CF_BY_PROJECT, Options::default()),
            ColumnFamilyDescriptor::new(CF_PROJECTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_CONFIG, Options::default()),
            ColumnFamilyDescriptor::new(CF_FACETS, Options::default()),
            ColumnFamilyDescriptor::new(CF_MEMORY_FACETS, Options::default()),
            ColumnFamilyDescriptor::new(CF_EDGES, Options::default()),
            ColumnFamilyDescriptor::new(CF_NODE_EDGES_OUT, Options::default()),
            ColumnFamilyDescriptor::new(CF_NODE_EDGES_IN, Options::default()),
            ColumnFamilyDescriptor::new(CF_PROPAGATION_DELTAS, Options::default()),
            ColumnFamilyDescriptor::new(CF_PENDING_PROPAGATIONS, Options::default()),
            ColumnFamilyDescriptor::new(CF_APPLIED_PROPAGATIONS, Options::default()),
            ColumnFamilyDescriptor::new(CF_RULES, Options::default()),
            ColumnFamilyDescriptor::new(CF_RULE_STATS, Options::default()),
            ColumnFamilyDescriptor::new(CF_MEMORY_SUMMARIES, Options::default()),
            ColumnFamilyDescriptor::new(CF_MEMORY_KEYWORDS, Options::default()),
            ColumnFamilyDescriptor::new(CF_SCENES, Options::default()),
            ColumnFamilyDescriptor::new(CF_BY_SCENE, Options::default()),
            ColumnFamilyDescriptor::new(CF_BY_HEAT, Options::default()),
            ColumnFamilyDescriptor::new(CF_BY_THEME, Options::default()),
            ColumnFamilyDescriptor::new(CF_DREAM_RUNS, Options::default()),
        ];

        let db = DB::open_cf_descriptors(&options, path, cfs).context("Failed to open RocksDB")?;

        Ok(Self { db })
    }

    /// 获取 `memories` 列族句柄。
    pub fn cf_memories(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_MEMORIES)
            .context("Column family 'memories' not found")
    }

    /// 获取 `by_tag` 列族句柄。
    pub fn cf_by_tag(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_BY_TAG)
            .context("Column family 'by_tag' not found")
    }

    /// 获取 `deleted` 列族句柄。
    pub fn cf_deleted(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_DELETED)
            .context("Column family 'deleted' not found")
    }

    /// 获取 `importance` 列族句柄。
    pub fn cf_importance(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_IMPORTANCE)
            .context("Column family 'importance' not found")
    }

    /// 获取 `by_created` 列族句柄。
    pub fn cf_by_created(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_BY_CREATED)
            .context("Column family 'by_created' not found")
    }

    /// 获取 `by_project` 列族句柄。
    pub fn cf_by_project(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_BY_PROJECT)
            .context("Column family 'by_project' not found")
    }

    /// 获取 `projects` 列族句柄。
    pub fn cf_projects(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_PROJECTS)
            .context("Column family 'projects' not found")
    }

    /// 批量提交写操作（列族级原子写入）。
    ///
    /// 用于 save 这类一次写多列族的场景，避免中途崩溃留下部分索引。
    pub fn write_batch(&self, batch: rocksdb::WriteBatch) -> Result<()> {
        self.db.write(batch).context("Failed to write batch")
    }

    /// 获取 `config` 列族句柄。
    pub fn cf_config(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_CONFIG)
            .context("Column family 'config' not found")
    }

    /// 获取 `facets` 列族句柄。
    pub fn cf_facets(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_FACETS)
            .context("Column family 'facets' not found")
    }

    /// 获取 `memory_facets` 列族句柄。
    pub fn cf_memory_facets(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_MEMORY_FACETS)
            .context("Column family 'memory_facets' not found")
    }

    /// 获取 `edges` 列族句柄。
    pub fn cf_edges(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_EDGES)
            .context("Column family 'edges' not found")
    }

    /// 获取 `node_edges_out` 列族句柄。
    pub fn cf_node_edges_out(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_NODE_EDGES_OUT)
            .context("Column family 'node_edges_out' not found")
    }

    /// 获取 `node_edges_in` 列族句柄。
    pub fn cf_node_edges_in(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_NODE_EDGES_IN)
            .context("Column family 'node_edges_in' not found")
    }

    /// 获取 `propagation_deltas` 列族句柄。
    pub fn cf_propagation_deltas(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_PROPAGATION_DELTAS)
            .context("Column family 'propagation_deltas' not found")
    }

    /// 获取 `pending_propagations` 列族句柄。
    pub fn cf_pending_propagations(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_PENDING_PROPAGATIONS)
            .context("Column family 'pending_propagations' not found")
    }

    /// 获取 `applied_propagations` 列族句柄。
    pub fn cf_applied_propagations(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_APPLIED_PROPAGATIONS)
            .context("Column family 'applied_propagations' not found")
    }

    /// 获取 `rules` 列族句柄。
    pub fn cf_rules(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_RULES)
            .context("Column family 'rules' not found")
    }

    /// 获取 `rule_stats` 列族句柄。
    pub fn cf_rule_stats(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_RULE_STATS)
            .context("Column family 'rule_stats' not found")
    }

    /// 获取 `memory_summaries` 列族句柄。
    pub fn cf_memory_summaries(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_MEMORY_SUMMARIES)
            .context("Column family 'memory_summaries' not found")
    }

    /// 获取 `memory_keywords` 列族句柄。
    pub fn cf_memory_keywords(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_MEMORY_KEYWORDS)
            .context("Column family 'memory_keywords' not found")
    }

    /// 获取 `scenes` 列族句柄。
    pub fn cf_scenes(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_SCENES)
            .context("Column family 'scenes' not found")
    }

    /// 获取 `by_scene` 列族句柄。
    pub fn cf_by_scene(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_BY_SCENE)
            .context("Column family 'by_scene' not found")
    }

    /// 获取 `by_heat` 列族句柄。
    pub fn cf_by_heat(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_BY_HEAT)
            .context("Column family 'by_heat' not found")
    }

    /// 获取 `by_theme` 列族句柄。
    pub fn cf_by_theme(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_BY_THEME)
            .context("Column family 'by_theme' not found")
    }

    /// 获取 `dream_runs` 列族句柄。
    pub fn cf_dream_runs(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_DREAM_RUNS)
            .context("Column family 'dream_runs' not found")
    }

    /// 向指定列族写入键值对。
    pub fn put_cf(&self, cf: &rocksdb::ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        self.db
            .put_cf(cf, key, value)
            .context("Failed to put value")
    }

    /// 从指定列族读取键对应的值。
    pub fn get_cf(&self, cf: &rocksdb::ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get_cf(cf, key).context("Failed to get value")
    }

    /// 批量读取指定列族的多个 key，单次多路读取以减少往返。
    ///
    /// 返回结果与传入 key 顺序一一对应；单项错误以 `Err` 承载，不中断其余项。
    pub fn multi_get_cf<'a, I>(
        &self,
        cf: &rocksdb::ColumnFamily,
        keys: I,
    ) -> Vec<Result<Option<Vec<u8>>>>
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        self.db
            .multi_get_cf(keys.into_iter().map(|k| (cf, k)))
            .into_iter()
            .map(|r| r.context("Failed to multi_get value"))
            .collect()
    }

    /// 从指定列族删除键。
    pub fn delete_cf(&self, cf: &rocksdb::ColumnFamily, key: &[u8]) -> Result<()> {
        self.db.delete_cf(cf, key).context("Failed to delete value")
    }

    /// 获取指定列族的原始迭代器，用于全量遍历。
    pub fn iter_cf(&self, cf: &rocksdb::ColumnFamily) -> rocksdb::DBRawIterator<'_> {
        self.db.raw_iterator_cf(cf)
    }

    /// 统计列族内 key 数量。
    ///
    /// 仅迭代 key 并计数，不反序列化 value，比全量 list 更高效。
    /// 用于 Dream Gate 的记忆总数检查等需要频繁调用的场景。
    pub fn count_cf_keys(&self, cf: &rocksdb::ColumnFamily) -> Result<usize> {
        let mut count = 0usize;
        let mut iter = self.db.raw_iterator_cf(cf);
        iter.seek_to_first();
        while iter.valid() {
            count += 1;
            iter.next();
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rocksdb_open() {
        let dir = tempdir().unwrap();
        let store = RocksDBStore::open(dir.path()).unwrap();
        assert!(store.cf_memories().is_ok());
    }

    #[test]
    fn test_rocksdb_put_get() {
        let dir = tempdir().unwrap();
        let store = RocksDBStore::open(dir.path()).unwrap();

        let cf = store.cf_memories().unwrap();
        store.put_cf(cf, b"test_key", b"test_value").unwrap();

        let value = store.get_cf(cf, b"test_key").unwrap();
        assert_eq!(value, Some(b"test_value".to_vec()));
    }
}
