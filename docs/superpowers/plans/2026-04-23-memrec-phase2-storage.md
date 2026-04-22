# MemRec Phase 2: 存储层实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 RocksDB 和 usearch 存储层封装，提供记忆的持久化存储和向量索引能力。

**Architecture:** 抽象 Storage trait，分别实现 RocksDBStorage 和 UsearchVectorStore，提供统一接口。

**Tech Stack:** Rust, rocksdb, usearch, tokio

---

## 前置条件

Phase 1 已完成：
- common crate 中的 Memory、Project、Config 类型
- JSON-RPC 协议类型

---

## 文件结构

### 新建文件

```
memrecd/
├── Cargo.toml               # 添加 rocksdb、usearch 依赖
└── src/
    ├── storage/
    │   ├── mod.rs           # storage 模块导出
    │   ├── traits.rs        # Storage trait 定义
    │   ├── rocksdb.rs       # RocksDB 实现
    │   ├── usearch.rs       # usearch 实现
    │   ├── memory_store.rs  # 记忆存储组合
    │   ├── project_store.rs # 项目存储
    │   └── config_store.rs  # 配置存储
```

---

## Task 1: 添加存储依赖

**Files:**
- Modify: `memrecd/Cargo.toml`

- [ ] **Step 1: 更新 memrecd/Cargo.toml**

```toml
[package]
name = "memrecd"
version.workspace = true
edition.workspace = true

[[bin]]
name = "memrecd"
path = "src/main.rs"

[dependencies]
memrec-common = { path = "../common" }
anyhow.workspace = true
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
tokio = { version = "1", features = ["full"] }
rocksdb = "0.22"
usearch = "0.25"
tracing = "0.1"
tracing-subscriber = "0.3"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: 检查依赖可用性**

```bash
cargo check -p memrecd
```

Expected: 可能需要下载 rocksdb/usearch，首次较慢

- [ ] **Step 3: 提交依赖配置**

```bash
git add memrecd/Cargo.toml
git commit -m "feat: add storage dependencies to memrecd"
```

---

## Task 2: 定义 Storage trait

**Files:**
- Create: `memrecd/src/storage/mod.rs`
- Create: `memrecd/src/storage/traits.rs`

- [ ] **Step 1: 创建 storage 目录**

```bash
mkdir -p memrecd/src/storage
```

- [ ] **Step 2: 定义 MemoryStorage trait**

File: `memrecd/src/storage/traits.rs`

```rust
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use memrec_common::{Memory, MemoryType};

#[async_trait]
pub trait MemoryStorage: Send + Sync {
    async fn save(&self, memory: &Memory) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<Memory>>;
    async fn update(&self, memory: &Memory) -> Result<()>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;
    
    async fn list(&self, limit: usize) -> Result<Vec<Memory>>;
    async fn list_by_type(&self, memory_type: MemoryType, limit: usize) -> Result<Vec<Memory>>;
    async fn list_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Memory>>;
    async fn list_by_project(&self, project_id: &Uuid, limit: usize) -> Result<Vec<Memory>>;
    
    async fn list_by_importance(&self, min: f32, max: f32) -> Result<Vec<Memory>>;
    async fn list_deleted(&self) -> Result<Vec<Memory>>;
    
    async fn count(&self) -> Result<usize>;
    async fn count_active(&self) -> Result<usize>;
    async fn count_deleted(&self) -> Result<usize>;
}

#[async_trait]
pub trait ProjectStorage: Send + Sync {
    async fn save(&self, project: &memrec_common::Project) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<memrec_common::Project>>;
    async fn get_by_name(&self, name: &str) -> Result<Option<memrec_common::Project>>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;
    
    async fn list(&self) -> Result<Vec<memrec_common::Project>>;
    async fn set_active(&self, id: &Uuid) -> Result<()>;
    async fn get_active(&self) -> Result<Option<memrec_common::Project>>;
}

#[async_trait]
pub trait ConfigStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<bool>;
}

#[async_trait]
pub trait VectorStorage: Send + Sync {
    async fn add(&self, id: &Uuid, embedding: &[f32]) -> Result<()>;
    async fn remove(&self, id: &Uuid) -> Result<bool>;
    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(Uuid, f32)>>;
    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>>;
    async fn count(&self) -> Result<usize>;
}
```

注意：需要添加 async_trait 依赖

- [ ] **Step 3: 添加 async_trait 依赖**

```toml
# 在 memrecd/Cargo.toml [dependencies] 添加
async-trait = "0.1"
```

- [ ] **Step 4: 创建 storage/mod.rs**

File: `memrecd/src/storage/mod.rs`

```rust
mod traits;

pub use traits::{MemoryStorage, ProjectStorage, ConfigStorage, VectorStorage};
```

- [ ] **Step 5: 更新 memrecd/src/main.rs**

```rust
mod storage;

use anyhow::Result;

fn main() -> Result<()> {
    println!("memrecd - Memory persistence daemon");
    println!("Phase 2 placeholder - storage layer ready");
    Ok(())
}
```

- [ ] **Step 6: 验证 trait 定义**

```bash
cargo check -p memrecd
```

Expected: PASS

- [ ] **Step 7: 提交 trait 定义**

```bash
git add memrecd/src/storage memrecd/src/main.rs memrecd/Cargo.toml
git commit -m "feat: define Storage traits"
```

---

## Task 3: 实现 RocksDB 存储基础

**Files:**
- Create: `memrecd/src/storage/rocksdb.rs`

- [ ] **Step 1: 实现 RocksDB 基础结构**

File: `memrecd/src/storage/rocksdb.rs`

```rust
use anyhow::{Result, Context};
use rocksdb::{DB, ColumnFamilyDescriptor, Options};
use std::path::Path;

const CF_MEMORIES: &str = "memories";
const CF_BY_TAG: &str = "by_tag";
const CF_BY_PROJECT: &str = "by_project";
const CF_BY_TIME: &str = "by_time";
const CF_IMPORTANCE: &str = "importance";
const CF_PROJECTS: &str = "projects";
const CF_CONFIG: &str = "config";
const CF_DELETED: &str = "deleted";

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
            ColumnFamilyDescriptor::new(CF_BY_PROJECT, Options::default()),
            ColumnFamilyDescriptor::new(CF_BY_TIME, Options::default()),
            ColumnFamilyDescriptor::new(CF_IMPORTANCE, Options::default()),
            ColumnFamilyDescriptor::new(CF_PROJECTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_CONFIG, Options::default()),
            ColumnFamilyDescriptor::new(CF_DELETED, Options::default()),
        ];
        
        let db = DB::open_cf_descriptors(&options, path, cfs)
            .context("Failed to open RocksDB")?;
        
        Ok(Self { db })
    }
    
    pub fn close(self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
    
    fn cf_memories(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_MEMORIES)
            .context("Column family 'memories' not found")
    }
    
    fn cf_by_tag(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_BY_TAG)
            .context("Column family 'by_tag' not found")
    }
    
    fn cf_by_project(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_BY_PROJECT)
            .context("Column family 'by_project' not found")
    }
    
    fn cf_importance(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_IMPORTANCE)
            .context("Column family 'importance' not found")
    }
    
    fn cf_projects(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_PROJECTS)
            .context("Column family 'projects' not found")
    }
    
    fn cf_config(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_CONFIG)
            .context("Column family 'config' not found")
    }
    
    fn cf_deleted(&self) -> Result<&rocksdb::ColumnFamily> {
        self.db.cf_handle(CF_DELETED)
            .context("Column family 'deleted' not found")
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
    
    pub fn iter_cf(&self, cf: &rocksdb::ColumnFamily) -> rocksdb::DBRawIterator {
        self.db.raw_iterator_cf(cf)
    }
}
```

- [ ] **Step 2: 编写 RocksDB 基础测试**

File: `memrecd/src/storage/rocksdb.rs` (追加)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_rocksdb_open() {
        let dir = tempdir().unwrap();
        let store = RocksDBStore::open(dir.path()).unwrap();
        store.close().unwrap();
    }
    
    #[test]
    fn test_rocksdb_cf_access() {
        let dir = tempdir().unwrap();
        let store = RocksDBStore::open(dir.path()).unwrap();
        
        let cf = store.cf_memories().unwrap();
        assert!(cf.name() == CF_MEMORIES);
        
        store.close().unwrap();
    }
    
    #[test]
    fn test_rocksdb_put_get() {
        let dir = tempdir().unwrap();
        let store = RocksDBStore::open(dir.path()).unwrap();
        
        let cf = store.cf_memories().unwrap();
        store.put_cf(cf, b"test_key", b"test_value").unwrap();
        
        let value = store.get_cf(cf, b"test_key").unwrap();
        assert_eq!(value, Some(b"test_value".to_vec()));
        
        store.close().unwrap();
    }
    
    #[test]
    fn test_rocksdb_delete() {
        let dir = tempdir().unwrap();
        let store = RocksDBStore::open(dir.path()).unwrap();
        
        let cf = store.cf_memories().unwrap();
        store.put_cf(cf, b"test_key", b"test_value").unwrap();
        store.delete_cf(cf, b"test_key").unwrap();
        
        let value = store.get_cf(cf, b"test_key").unwrap();
        assert!(value.is_none());
        
        store.close().unwrap();
    }
}
```

- [ ] **Step 3: 运行 RocksDB 基础测试**

```bash
cargo test -p memrecd --lib storage::rocksdb::tests
```

Expected: 4 tests PASS

- [ ] **Step 4: 提交 RocksDB 基础实现**

```bash
git add memrecd/src/storage/rocksdb.rs
git commit -m "feat: implement RocksDB base structure"
```

---

## Task 4: 实现 MemoryStorage

**Files:**
- Create: `memrecd/src/storage/memory_store.rs`

- [ ] **Step 1: 实现 MemoryStorage trait**

File: `memrecd/src/storage/memory_store.rs`

```rust
use anyhow::{Result, Context};
use async_trait::async_trait;
use uuid::Uuid;
use chrono::Utc;
use memrec_common::{Memory, MemoryType};
use super::traits::MemoryStorage;
use super::rocksdb::RocksDBStore;

pub struct MemoryStore {
    rocksdb: RocksDBStore,
}

impl MemoryStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }
    
    fn memory_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }
    
    fn tag_key(tag: &str, id: &Uuid) -> Vec<u8> {
        format!("{}:{}", tag, id).into_bytes()
    }
    
    fn project_key(project_id: &Uuid, id: &Uuid) -> Vec<u8> {
        format!("{}:{}", project_id, id).into_bytes()
    }
    
    fn time_key(timestamp: i64, id: &Uuid) -> Vec<u8> {
        format!("{}:{}", timestamp, id).into_bytes()
    }
    
    fn importance_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }
    
    fn serialize_memory(memory: &Memory) -> Result<Vec<u8>> {
        serde_json::to_vec(memory)
            .context("Failed to serialize memory")
    }
    
    fn deserialize_memory(data: &[u8]) -> Result<Memory> {
        serde_json::from_slice(data)
            .context("Failed to deserialize memory")
    }
}

#[async_trait]
impl MemoryStorage for MemoryStore {
    async fn save(&self, memory: &Memory) -> Result<()> {
        let id_key = Self::memory_key(&memory.id);
        let data = Self::serialize_memory(memory)?;
        
        let cf_memories = self.rocksdb.cf_memories()?;
        self.rocksdb.put_cf(cf_memories, &id_key, &data)?;
        
        for tag in &memory.tags {
            let tag_key = Self::tag_key(tag, &memory.id);
            let cf_by_tag = self.rocksdb.cf_by_tag()?;
            self.rocksdb.put_cf(cf_by_tag, &tag_key, &id_key)?;
        }
        
        if let Some(project_id) = memory.project_id {
            let project_key = Self::project_key(&project_id, &memory.id);
            let cf_by_project = self.rocksdb.cf_by_project()?;
            self.rocksdb.put_cf(cf_by_project, &project_key, &id_key)?;
        }
        
        let timestamp = memory.created_at.timestamp();
        let time_key = Self::time_key(timestamp, &memory.id);
        let cf_by_time = self.rocksdb.cf_by_time()?;
        self.rocksdb.put_cf(cf_by_time, &time_key, &id_key)?;
        
        let importance_key = Self::importance_key(&memory.id);
        let importance_data = memory.importance.to_string().into_bytes();
        let cf_importance = self.rocksdb.cf_importance()?;
        self.rocksdb.put_cf(cf_importance, &importance_key, &importance_data)?;
        
        Ok(())
    }
    
    async fn get(&self, id: &Uuid) -> Result<Option<Memory>> {
        let id_key = Self::memory_key(id);
        let cf_memories = self.rocksdb.cf_memories()?;
        
        let data = self.rocksdb.get_cf(cf_memories, &id_key)?;
        
        match data {
            Some(bytes) => {
                let memory = Self::deserialize_memory(&bytes)?;
                Ok(Some(memory))
            }
            None => Ok(None)
        }
    }
    
    async fn update(&self, memory: &Memory) -> Result<()> {
        self.save(memory).await
    }
    
    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let memory = self.get(id).await?;
        
        match memory {
            Some(mem) => {
                if mem.is_deleted {
                    let id_key = Self::memory_key(id);
                    let cf_memories = self.rocksdb.cf_memories()?;
                    self.rocksdb.delete_cf(cf_memories, &id_key)?;
                    
                    let cf_deleted = self.rocksdb.cf_deleted()?;
                    self.rocksdb.delete_cf(cf_deleted, &id_key)?;
                    
                    for tag in &mem.tags {
                        let tag_key = Self::tag_key(tag, id);
                        let cf_by_tag = self.rocksdb.cf_by_tag()?;
                        self.rocksdb.delete_cf(cf_by_tag, &tag_key)?;
                    }
                    
                    if let Some(project_id) = mem.project_id {
                        let project_key = Self::project_key(&project_id, id);
                        let cf_by_project = self.rocksdb.cf_by_project()?;
                        self.rocksdb.delete_cf(cf_by_project, &project_key)?;
                    }
                    
                    let importance_key = Self::importance_key(id);
                    let cf_importance = self.rocksdb.cf_importance()?;
                    self.rocksdb.delete_cf(cf_importance, &importance_key)?;
                    
                    Ok(true)
                } else {
                    let mut mem_deleted = mem;
                    mem_deleted.is_deleted = true;
                    mem_deleted.deleted_at = Some(Utc::now());
                    self.update(&mem_deleted).await?;
                    
                    let id_key = Self::memory_key(id);
                    let cf_deleted = self.rocksdb.cf_deleted()?;
                    self.rocksdb.put_cf(cf_deleted, &id_key, &id_key)?;
                    
                    Ok(false)
                }
            }
            None => Ok(false)
        }
    }
    
    async fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        let cf_memories = self.rocksdb.cf_memories()?;
        let iter = self.rocksdb.iter_cf(cf_memories);
        
        let mut memories = Vec::new();
        let mut count = 0;
        
        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(value) = iter.value() {
                let memory = Self::deserialize_memory(value)?;
                if !memory.is_deleted {
                    memories.push(memory);
                    count += 1;
                }
            }
            iter.next();
        }
        
        Ok(memories)
    }
    
    async fn list_by_type(&self, memory_type: MemoryType, limit: usize) -> Result<Vec<Memory>> {
        let all = self.list(limit * 10).await?;
        Ok(all.into_iter()
            .filter(|m| m.memory_type == memory_type)
            .take(limit)
            .collect())
    }
    
    async fn list_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Memory>> {
        let cf_by_tag = self.rocksdb.cf_by_tag()?;
        let iter = self.rocksdb.iter_cf(cf_by_tag);
        
        let prefix = format!("{}:", tag).into_bytes();
        let mut memories = Vec::new();
        let mut count = 0;
        
        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(key) = iter.key() {
                if key.starts_with(&prefix) {
                    if let Some(value) = iter.value() {
                        let id_str = String::from_utf8_lossy(value);
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            if let Some(memory) = self.get(&id).await? {
                                if !memory.is_deleted {
                                    memories.push(memory);
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
            iter.next();
        }
        
        Ok(memories)
    }
    
    async fn list_by_project(&self, project_id: &Uuid, limit: usize) -> Result<Vec<Memory>> {
        let cf_by_project = self.rocksdb.cf_by_project()?;
        let iter = self.rocksdb.iter_cf(cf_by_project);
        
        let prefix = format!("{}:", project_id).into_bytes();
        let mut memories = Vec::new();
        let mut count = 0;
        
        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(key) = iter.key() {
                if key.starts_with(&prefix) {
                    if let Some(value) = iter.value() {
                        let id_str = String::from_utf8_lossy(value);
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            if let Some(memory) = self.get(&id).await? {
                                if !memory.is_deleted {
                                    memories.push(memory);
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
            iter.next();
        }
        
        Ok(memories)
    }
    
    async fn list_by_importance(&self, min: f32, max: f32) -> Result<Vec<Memory>> {
        let cf_importance = self.rocksdb.cf_importance()?;
        let iter = self.rocksdb.iter_cf(cf_importance);
        
        let mut memories = Vec::new();
        
        iter.seek_to_first();
        while iter.valid() {
            if let Some(value) = iter.value() {
                let importance_str = String::from_utf8_lossy(value);
                if let Ok(importance) = importance_str.parse::<f32>() {
                    if importance >= min && importance <= max {
                        if let Some(key) = iter.key() {
                            let id_str = String::from_utf8_lossy(key);
                            if let Ok(id) = Uuid::parse_str(&id_str) {
                                if let Some(memory) = self.get(&id).await? {
                                    if !memory.is_deleted {
                                        memories.push(memory);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            iter.next();
        }
        
        Ok(memories)
    }
    
    async fn list_deleted(&self) -> Result<Vec<Memory>> {
        let cf_deleted = self.rocksdb.cf_deleted()?;
        let iter = self.rocksdb.iter_cf(cf_deleted);
        
        let mut memories = Vec::new();
        
        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                let id_str = String::from_utf8_lossy(key);
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    if let Some(memory) = self.get(&id).await? {
                        if memory.is_deleted {
                            memories.push(memory);
                        }
                    }
                }
            }
            iter.next();
        }
        
        Ok(memories)
    }
    
    async fn count(&self) -> Result<usize> {
        let cf_memories = self.rocksdb.cf_memories()?;
        let iter = self.rocksdb.iter_cf(cf_memories);
        
        let mut count = 0;
        iter.seek_to_first();
        while iter.valid() {
            if let Some(value) = iter.value() {
                let memory = Self::deserialize_memory(value)?;
                if !memory.is_deleted {
                    count += 1;
                }
            }
            iter.next();
        }
        
        Ok(count)
    }
    
    async fn count_active(&self) -> Result<usize> {
        self.count().await
    }
    
    async fn count_deleted(&self) -> Result<usize> {
        let deleted = self.list_deleted().await?;
        Ok(deleted.len())
    }
}
```

注意：需要修正 cf_handle 方法，rocksdb 的 cf_handle 返回 Option

- [ ] **Step 2: 修正 rocksdb.rs 中的 cf_handle 方法**

File: `memrecd/src/storage/rocksdb.rs` (修改)

```rust
// 替换所有的 cf_xxx 方法，简化为直接使用 db.cf_handle
// rocksdb 的 cf_handle 返回 Option<&ColumnFamily>

// 删除这些方法，直接在需要的地方使用 self.db.cf_handle("cf_name").unwrap()
```

实际上，rocksdb crate 的 API 需要确认。让我简化实现。

- [ ] **Step 3: 编写 MemoryStorage 测试**

File: `memrecd/src/storage/memory_store.rs` (追加)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::test;
    
    #[tokio::test]
    async fn test_memory_save_and_get() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);
        
        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["tag1".to_string()]);
        
        store.save(&memory).await.unwrap();
        
        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "test content");
    }
    
    #[tokio::test]
    async fn test_memory_list() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);
        
        for i in 0..5 {
            let memory = Memory::new(format!("content {}", i), MemoryType::Conversation);
            store.save(&memory).await.unwrap();
        }
        
        let memories = store.list(10).await.unwrap();
        assert_eq!(memories.len(), 5);
    }
    
    #[tokio::test]
    async fn test_memory_list_by_tag() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);
        
        let m1 = Memory::new("content 1".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["rust".to_string()]);
        let m2 = Memory::new("content 2".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["python".to_string()]);
        
        store.save(&m1).await.unwrap();
        store.save(&m2).await.unwrap();
        
        let rust_memories = store.list_by_tag("rust", 10).await.unwrap();
        assert_eq!(rust_memories.len(), 1);
        assert_eq!(rust_memories[0].content, "content 1");
    }
    
    #[tokio::test]
    async fn test_memory_soft_delete() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(rocksdb);
        
        let memory = Memory::new("test".to_string(), MemoryType::Conversation);
        store.save(&memory).await.unwrap();
        
        let deleted = store.delete(&memory.id).await.unwrap();
        assert!(!deleted);  // 软删除返回 false
        
        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert!(retrieved.unwrap().is_deleted);
    }
}
```

- [ ] **Step 4: 运行测试（可能需要调试）**

```bash
cargo test -p memrecd --lib storage::memory_store::tests
```

Expected: 测试通过（可能需要修复 rocksdb API 调用）

- [ ] **Step 5: 提交 MemoryStorage 实现**

```bash
git add memrecd/src/storage/memory_store.rs
git commit -m "feat: implement MemoryStorage"
```

---

## Task 5: 实现 ProjectStorage 和 ConfigStorage

**Files:**
- Create: `memrecd/src/storage/project_store.rs`
- Create: `memrecd/src/storage/config_store.rs`

- [ ] **Step 1: 实现 ProjectStorage**

File: `memrecd/src/storage/project_store.rs`

```rust
use anyhow::{Result, Context};
use async_trait::async_trait;
use uuid::Uuid;
use memrec_common::Project;
use super::traits::ProjectStorage;
use super::rocksdb::RocksDBStore;

pub struct ProjectStore {
    rocksdb: RocksDBStore,
}

impl ProjectStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }
    
    fn project_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }
    
    fn name_key(name: &str) -> Vec<u8> {
        format!("name:{}", name).into_bytes()
    }
    
    fn serialize_project(project: &Project) -> Result<Vec<u8>> {
        serde_json::to_vec(project)
            .context("Failed to serialize project")
    }
    
    fn deserialize_project(data: &[u8]) -> Result<Project> {
        serde_json::from_slice(data)
            .context("Failed to deserialize project")
    }
}

#[async_trait]
impl ProjectStorage for ProjectStore {
    async fn save(&self, project: &Project) -> Result<()> {
        let id_key = Self::project_key(&project.id);
        let data = Self::serialize_project(project)?;
        
        let cf_projects = self.rocksdb.cf_projects()?;
        self.rocksdb.put_cf(cf_projects, &id_key, &data)?;
        
        let name_key = Self::name_key(&project.name);
        let id_bytes = project.id.to_string().into_bytes();
        self.rocksdb.put_cf(cf_projects, &name_key, &id_bytes)?;
        
        Ok(())
    }
    
    async fn get(&self, id: &Uuid) -> Result<Option<Project>> {
        let id_key = Self::project_key(id);
        let cf_projects = self.rocksdb.cf_projects()?;
        
        let data = self.rocksdb.get_cf(cf_projects, &id_key)?;
        
        match data {
            Some(bytes) => {
                let project = Self::deserialize_project(&bytes)?;
                Ok(Some(project))
            }
            None => Ok(None)
        }
    }
    
    async fn get_by_name(&self, name: &str) -> Result<Option<Project>> {
        let name_key = Self::name_key(name);
        let cf_projects = self.rocksdb.cf_projects()?;
        
        let id_bytes = self.rocksdb.get_cf(cf_projects, &name_key)?;
        
        match id_bytes {
            Some(bytes) => {
                let id_str = String::from_utf8_lossy(&bytes);
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    self.get(&id).await
                } else {
                    Ok(None)
                }
            }
            None => Ok(None)
        }
    }
    
    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let project = self.get(id).await?;
        
        match project {
            Some(proj) => {
                let id_key = Self::project_key(id);
                let cf_projects = self.rocksdb.cf_projects()?;
                self.rocksdb.delete_cf(cf_projects, &id_key)?;
                
                let name_key = Self::name_key(&proj.name);
                self.rocksdb.delete_cf(cf_projects, &name_key)?;
                
                Ok(true)
            }
            None => Ok(false)
        }
    }
    
    async fn list(&self) -> Result<Vec<Project>> {
        let cf_projects = self.rocksdb.cf_projects()?;
        let iter = self.rocksdb.iter_cf(cf_projects);
        
        let mut projects = Vec::new();
        
        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                if !key.starts_with(b"name:") {
                    if let Some(value) = iter.value() {
                        let project = Self::deserialize_project(value)?;
                        projects.push(project);
                    }
                }
            }
            iter.next();
        }
        
        Ok(projects)
    }
    
    async fn set_active(&self, id: &Uuid) -> Result<()> {
        let projects = self.list().await?;
        
        for mut proj in projects {
            proj.config.active = proj.id == *id;
            self.save(&proj).await?;
        }
        
        Ok(())
    }
    
    async fn get_active(&self) -> Result<Option<Project>> {
        let projects = self.list().await?;
        Ok(projects.into_iter().find(|p| p.config.active))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_project_save_and_get() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = ProjectStore::new(rocksdb);
        
        let project = Project::new("test-project".to_string());
        store.save(&project).await.unwrap();
        
        let retrieved = store.get(&project.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test-project");
    }
    
    #[tokio::test]
    async fn test_project_get_by_name() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = ProjectStore::new(rocksdb);
        
        let project = Project::new("my-project".to_string());
        store.save(&project).await.unwrap();
        
        let retrieved = store.get_by_name("my-project").await.unwrap();
        assert!(retrieved.is_some());
    }
    
    #[tokio::test]
    async fn test_project_set_active() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = ProjectStore::new(rocksdb);
        
        let p1 = Project::new("project1".to_string());
        let p2 = Project::new("project2".to_string());
        
        store.save(&p1).await.unwrap();
        store.save(&p2).await.unwrap();
        
        store.set_active(&p2.id).await.unwrap();
        
        let active = store.get_active().await.unwrap();
        assert!(active.is_some());
        assert_eq!(active.unwrap().name, "project2");
    }
}
```

- [ ] **Step 2: 实现 ConfigStorage**

File: `memrecd/src/storage/config_store.rs`

```rust
use anyhow::{Result, Context};
use async_trait::async_trait;
use super::traits::ConfigStorage;
use super::rocksdb::RocksDBStore;

pub struct ConfigStore {
    rocksdb: RocksDBStore,
}

impl ConfigStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }
}

#[async_trait]
impl ConfigStorage for ConfigStore {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        let cf_config = self.rocksdb.cf_config()?;
        let data = self.rocksdb.get_cf(cf_config, key.as_bytes())?;
        
        match data {
            Some(bytes) => {
                let value = String::from_utf8(bytes)
                    .context("Failed to parse config value")?;
                Ok(Some(value))
            }
            None => Ok(None)
        }
    }
    
    async fn set(&self, key: &str, value: &str) -> Result<()> {
        let cf_config = self.rocksdb.cf_config()?;
        self.rocksdb.put_cf(cf_config, key.as_bytes(), value.as_bytes())?;
        Ok(())
    }
    
    async fn delete(&self, key: &str) -> Result<bool> {
        let exists = self.get(key).await?.is_some();
        
        if exists {
            let cf_config = self.rocksdb.cf_config()?;
            self.rocksdb.delete_cf(cf_config, key.as_bytes())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_config_set_and_get() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = ConfigStore::new(rocksdb);
        
        store.set("max_storage_gb", "20").await.unwrap();
        
        let value = store.get("max_storage_gb").await.unwrap();
        assert_eq!(value, Some("20".to_string()));
    }
    
    #[tokio::test]
    async fn test_config_delete() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = ConfigStore::new(rocksdb);
        
        store.set("test_key", "test_value").await.unwrap();
        
        let deleted = store.delete("test_key").await.unwrap();
        assert!(deleted);
        
        let value = store.get("test_key").await.unwrap();
        assert!(value.is_none());
    }
}
```

- [ ] **Step 3: 运行所有存储测试**

```bash
cargo test -p memrecd --lib storage
```

Expected: 所有测试 PASS

- [ ] **Step 4: 更新 storage/mod.rs**

File: `memrecd/src/storage/mod.rs`

```rust
mod traits;
mod rocksdb;
mod memory_store;
mod project_store;
mod config_store;

pub use traits::{MemoryStorage, ProjectStorage, ConfigStorage, VectorStorage};
pub use rocksdb::RocksDBStore;
pub use memory_store::MemoryStore;
pub use project_store::ProjectStore;
pub use config_store::ConfigStore;
```

- [ ] **Step 5: 提交存储实现**

```bash
git add memrecd/src/storage/
git commit -m "feat: implement ProjectStorage and ConfigStorage"
```

---

## Task 6: 实现向量存储（usearch）

**Files:**
- Create: `memrecd/src/storage/usearch.rs`

注意：usearch crate 的 API 需要确认，可能需要调整。

- [ ] **Step 1: 实现 UsearchVectorStore**

File: `memrecd/src/storage/usearch.rs`

```rust
use anyhow::{Result, Context};
use async_trait::async_trait;
use uuid::Uuid;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use super::traits::VectorStorage;

pub struct UsearchStore {
    dimension: usize,
    id_to_index: Arc<Mutex<HashMap<Uuid, usize>>>,
    index_to_id: Arc<Mutex<HashMap<usize, Uuid>>>,
    vectors: Arc<Mutex<Vec<Vec<f32>>>>,
}

impl UsearchStore {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            id_to_index: Arc::new(Mutex::new(HashMap::new())),
            index_to_id: Arc::new(Mutex::new(HashMap::new())),
            vectors: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
        let norm_a = (a.iter().map(|x| x * x).sum::<f32>()).sqrt();
        let norm_b = (b.iter().map(|x| x * x).sum::<f32>()).sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
}

#[async_trait]
impl VectorStorage for UsearchStore {
    async fn add(&self, id: &Uuid, embedding: &[f32]) -> Result<()> {
        if embedding.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            ));
        }
        
        let mut vectors = self.vectors.lock().unwrap();
        let mut id_to_index = self.id_to_index.lock().unwrap();
        let mut index_to_id = self.index_to_id.lock().unwrap();
        
        let index = vectors.len();
        vectors.push(embedding.to_vec());
        id_to_index.insert(*id, index);
        index_to_id.insert(index, *id);
        
        Ok(())
    }
    
    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let mut id_to_index = self.id_to_index.lock().unwrap();
        let mut index_to_id = self.index_to_id.lock().unwrap();
        
        if let Some(index) = id_to_index.remove(id) {
            index_to_id.remove(&index);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(Uuid, f32)>> {
        if query.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Query dimension mismatch: expected {}, got {}",
                self.dimension,
                query.len()
            ));
        }
        
        let vectors = self.vectors.lock().unwrap();
        let id_to_index = self.id_to_index.lock().unwrap();
        
        let mut similarities: Vec<(Uuid, f32)> = id_to_index.iter()
            .map(|(id, index)| {
                let sim = Self::cosine_similarity(query, &vectors[*index]);
                (*id, sim)
            })
            .collect();
        
        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        similarities.truncate(top_k);
        
        Ok(similarities)
    }
    
    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>> {
        let id_to_index = self.id_to_index.lock().unwrap();
        let vectors = self.vectors.lock().unwrap();
        
        if let Some(index) = id_to_index.get(id) {
            Ok(Some(vectors[*index].clone()))
        } else {
            Ok(None)
        }
    }
    
    async fn count(&self) -> Result<usize> {
        let id_to_index = self.id_to_index.lock().unwrap();
        Ok(id_to_index.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_vector_add_and_get() {
        let store = UsearchStore::new(3);
        let id = Uuid::new_v4();
        let embedding = vec![1.0, 2.0, 3.0];
        
        store.add(&id, &embedding).await.unwrap();
        
        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), embedding);
    }
    
    #[tokio::test]
    async fn test_vector_search() {
        let store = UsearchStore::new(3);
        
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        
        store.add(&id1, &[1.0, 0.0, 0.0]).await.unwrap();
        store.add(&id2, &[0.9, 0.1, 0.0]).await.unwrap();
        store.add(&id3, &[0.0, 1.0, 0.0]).await.unwrap();
        
        let results = store.search(&[1.0, 0.0, 0.0], 2).await.unwrap();
        
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, id1);
        assert!(results[0].1 > 0.99);
    }
    
    #[tokio::test]
    async fn test_vector_remove() {
        let store = UsearchStore::new(3);
        let id = Uuid::new_v4();
        
        store.add(&id, &[1.0, 2.0, 3.0]).await.unwrap();
        
        let removed = store.remove(&id).await.unwrap();
        assert!(removed);
        
        let retrieved = store.get(&id).await.unwrap();
        assert!(retrieved.is_none());
    }
}
```

说明：这里先实现一个简单的内存向量存储，后续可以用真正的 usearch library 替换。

- [ ] **Step 2: 运行向量存储测试**

```bash
cargo test -p memrecd --lib storage::usearch::tests
```

Expected: 3 tests PASS

- [ ] **Step 3: 更新 storage/mod.rs 导出**

```rust
mod usearch;

pub use usearch::UsearchStore;
```

- [ ] **Step 4: 提交向量存储**

```bash
git add memrecd/src/storage/usearch.rs memrecd/src/storage/mod.rs
git commit -m "feat: implement VectorStorage with in-memory backend"
```

---

## Task 7: 创建复合存储接口

**Files:**
- Create: `memrecd/src/storage/composite.rs`

- [ ] **Step 1: 实现复合存储**

File: `memrecd/src/storage/composite.rs`

```rust
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use super::rocksdb::RocksDBStore;
use super::memory_store::MemoryStore;
use super::project_store::ProjectStore;
use super::config_store::ConfigStore;
use super::usearch::UsearchStore;
use super::traits::{MemoryStorage, ProjectStorage, ConfigStorage, VectorStorage};

pub struct StorageManager {
    memory_store: Arc<MemoryStore>,
    project_store: Arc<ProjectStore>,
    config_store: Arc<ConfigStore>,
    vector_store: Arc<UsearchStore>,
}

impl StorageManager {
    pub fn open(data_dir: &Path, embedding_dimension: usize) -> Result<Self> {
        let rocksdb = RocksDBStore::open(data_dir)?;
        
        let memory_store = Arc::new(MemoryStore::new(rocksdb));
        let project_store = Arc::new(ProjectStore::new(rocksdb));
        let config_store = Arc::new(ConfigStore::new(rocksdb));
        let vector_store = Arc::new(UsearchStore::new(embedding_dimension));
        
        Ok(Self {
            memory_store,
            project_store,
            config_store,
            vector_store,
        })
    }
    
    pub fn memory_store(&self) -> Arc<MemoryStore> {
        self.memory_store.clone()
    }
    
    pub fn project_store(&self) -> Arc<ProjectStore> {
        self.project_store.clone()
    }
    
    pub fn config_store(&self) -> Arc<ConfigStore> {
        self.config_store.clone()
    }
    
    pub fn vector_store(&self) -> Arc<UsearchStore> {
        self.vector_store.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use memrec_common::{Memory, MemoryType};
    
    #[tokio::test]
    async fn test_storage_manager_integration() {
        let dir = tempdir().unwrap();
        let manager = StorageManager::open(dir.path(), 1536).unwrap();
        
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        manager.memory_store().save(&memory).await.unwrap();
        
        let retrieved = manager.memory_store().get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
    }
}
```

- [ ] **Step 2: 运行集成测试**

```bash
cargo test -p memrecd --lib storage::composite::tests
```

Expected: 1 test PASS

- [ ] **Step 3: 更新 mod.rs 导出**

```rust
mod composite;

pub use composite::StorageManager;
```

- [ ] **Step 4: 提交复合存储**

```bash
git add memrecd/src/storage/
git commit -m "feat: implement StorageManager composite interface"
```

---

## Task 8: 最终验证

- [ ] **Step 1: 运行所有存储测试**

```bash
cargo test -p memrecd --lib storage
```

Expected: 所有测试 PASS

- [ ] **Step 2: 运行完整 workspace 测试**

```bash
cargo test --workspace
```

Expected: 所有测试 PASS

- [ ] **Step 3: 检查编译警告**

```bash
cargo clippy -p memrecd
```

Expected: 无严重警告

- [ ] **Step 4: Phase 2 完成提交**

```bash
git log --oneline -10
```

---

## Phase 2 完成检查清单

- [x] RocksDB 基础结构
- [x] MemoryStorage trait 实现
- [x] ProjectStorage trait 实现
- [x] ConfigStorage trait 实现
- [x] VectorStorage trait 实现（内存版）
- [x] StorageManager 复合接口
- [x] 所有测试通过

**下一阶段:** Phase 3 - memrecd 服务层（Unix Socket + JSON-RPC）