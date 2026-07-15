//! # RocksDB 底层封装
//!
//! 对 RocksDB 的列族（Column Family）操作进行封装，
//! 为上层 [`MemoryStore`] 和 [`RocksDBVectorStore`] 提供基础读写能力。
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
/// 项目信息列族
const CF_PROJECTS: &str = "projects";
/// 配置键值对列族
const CF_CONFIG: &str = "config";

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
            ColumnFamilyDescriptor::new(CF_PROJECTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_CONFIG, Options::default()),
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

    /// 获取 `projects` 列族句柄。
    pub fn cf_projects(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_PROJECTS)
            .context("Column family 'projects' not found")
    }

    /// 获取 `config` 列族句柄。
    pub fn cf_config(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(CF_CONFIG)
            .context("Column family 'config' not found")
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

    /// 从指定列族删除键。
    pub fn delete_cf(&self, cf: &rocksdb::ColumnFamily, key: &[u8]) -> Result<()> {
        self.db.delete_cf(cf, key).context("Failed to delete value")
    }

    /// 获取指定列族的原始迭代器，用于全量遍历。
    pub fn iter_cf(&self, cf: &rocksdb::ColumnFamily) -> rocksdb::DBRawIterator<'_> {
        self.db.raw_iterator_cf(cf)
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
