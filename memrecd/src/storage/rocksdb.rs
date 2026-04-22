use anyhow::{Result, Context};
use rocksdb::{DB, ColumnFamilyDescriptor, Options};
use std::path::Path;

const CF_MEMORIES: &str = "memories";
const CF_BY_TAG: &str = "by_tag";
const CF_DELETED: &str = "deleted";
const CF_IMPORTANCE: &str = "importance";
const CF_PROJECTS: &str = "projects";
const CF_CONFIG: &str = "config";

pub struct RocksDBStore {
    db: DB,
}

impl RocksDBStore {
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
        
        let db = DB::open_cf_descriptors(&options, path, cfs)
            .context("Failed to open RocksDB")?;
        
        Ok(Self { db })
    }
    
    pub fn cf_memories(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_MEMORIES)
            .context("Column family 'memories' not found")
    }
    
    pub fn cf_by_tag(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_BY_TAG)
            .context("Column family 'by_tag' not found")
    }
    
    pub fn cf_deleted(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_DELETED)
            .context("Column family 'deleted' not found")
    }
    
    pub fn cf_importance(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_IMPORTANCE)
            .context("Column family 'importance' not found")
    }
    
    pub fn cf_projects(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_PROJECTS)
            .context("Column family 'projects' not found")
    }
    
    pub fn cf_config(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_CONFIG)
            .context("Column family 'config' not found")
    }
    
    pub fn put_cf(&self, cf: &rocksdb::ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        self.db.put_cf(cf, key, value)
            .context("Failed to put value")
    }
    
    pub fn get_cf(&self, cf: &rocksdb::ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get_cf(cf, key)
            .context("Failed to get value")
    }
    
    pub fn delete_cf(&self, cf: &rocksdb::ColumnFamily, key: &[u8]) -> Result<()> {
        self.db.delete_cf(cf, key)
            .context("Failed to delete value")
    }
    
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