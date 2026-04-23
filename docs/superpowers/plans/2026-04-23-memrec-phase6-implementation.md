# MemRec Phase 6 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现项目记忆隔离和语义检索功能，使memrec成为AI-First的记忆系统

**Architecture:** 
- 项目检测：Git根目录 + `.mr_pid`文件自动管理
- 向量存储：Qdrant嵌入式模式替代内存存储
- Embedding：FastEmbed本地生成，CPU友好
- CLI：默认JSON输出，`--human`可选切换

**Tech Stack:** Rust, qdrant-client, fastembed, all-MiniLM-L6-v2 (384维)

---

## File Structure

### 新建文件

| 文件 | 职责 |
|------|------|
| `memrecd/src/project/mod.rs` | 项目检测模块入口 |
| `memrecd/src/project/detect.rs` | Git根目录检测 + `.mr_pid`文件管理 |
| `memrecd/src/embedding/mod.rs` | Embedding模块入口 |
| `memrecd/src/embedding/fastembed.rs` | FastEmbed本地生成实现 |
| `memrecd/src/storage/qdrant.rs` | Qdrant嵌入式向量存储 |
| `memrec/src/commands/search.rs` | 语义检索CLI命令 |
| `docs/skills/memrec-skill-phase6.md` | 更新的Skill文档 |

### 修改文件

| 文件 | 变更内容 |
|------|---------|
| `common/src/types/memory.rs` | 新增 `chunk_group_id`, `chunk_index`, `chunk_total` 字段 |
| `common/src/protocol/request.rs` | 新增 `SearchMemory`, `GetProjectInfo`, `MergeChunks` Action |
| `common/src/protocol/response.rs` | 新增 `SearchResult`, `ProjectInfoResult` 类型 |
| `memrecd/src/server/handler.rs` | 新增 SearchMemory, GetProjectInfo 处理 |
| `memrecd/src/storage/mod.rs` | 导出 QdrantVectorStore |
| `memrecd/src/storage/traits.rs` | 更新 VectorStorage trait |
| `memrec/src/commands/memory.rs` | 新增 `--global`, `--project-only` 参数 |
| `memrec/src/commands/mod.rs` | 导出 search 命令 |
| `memrec/src/main.rs` | 新增 search 子命令 |
| `Cargo.toml` (workspace) | 新增 qdrant-client, fastembed 依赖 |

---

## Phase 6.1: 项目ID检测 + `.mr_pid`

### Task 1: 新建项目检测模块

**Files:**
- Create: `memrecd/src/project/mod.rs`
- Create: `memrecd/src/project/detect.rs`
- Test: `memrecd/src/project/detect.rs` (inline tests)

- [ ] **Step 1: 创建模块入口文件**

```rust
// memrecd/src/project/mod.rs
pub mod detect;

pub use detect::{detect_project_id, ProjectIdFile};
```

- [ ] **Step 2: 创建项目检测实现文件**

```rust
// memrecd/src/project/detect.rs
use std::path::{Path, PathBuf};
use std::fs;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use anyhow::Result;

pub struct ProjectIdFile {
    pub project_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub project_name: Option<String>,
}

impl ProjectIdFile {
    pub fn new(project_name: Option<String>) -> Self {
        Self {
            project_id: Uuid::new_v4(),
            created_at: Utc::now(),
            project_name,
        }
    }
    
    pub fn parse(content: &str) -> Result<Self> {
        let lines = content.lines();
        let mut project_id: Option<Uuid> = None;
        let mut created_at: Option<DateTime<Utc>> = None;
        let mut project_name: Option<String> = None;
        
        for line in lines {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "memrec_project_id" => {
                        project_id = Some(Uuid::parse_str(value.trim())?);
                    }
                    "created_at" => {
                        created_at = Some(DateTime::parse_from_rfc3339(value.trim())?.with_timezone());
                    }
                    "project_name" => {
                        project_name = Some(value.trim().to_string());
                    }
                    _ => {}
                }
            }
        }
        
        match (project_id, created_at) {
            (Some(id), Some(at)) => Ok(Self {
                project_id: id,
                created_at: at,
                project_name,
            }),
            _ => anyhow::bail!("Invalid .mr_pid file format"),
        }
    }
    
    pub fn to_string(&self) -> String {
        let mut content = format!(
            "memrec_project_id={}\ncreated_at={}",
            self.project_id,
            self.created_at.to_rfc3339()
        );
        if let Some(name) = &self.project_name {
            content.push_str(&format!("\nproject_name={}", name));
        }
        content
    }
}

pub fn find_project_root() -> Result<PathBuf> {
    let current = std::env::current_dir()?;
    
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&current)
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(PathBuf::from(path));
        }
    }
    
    Ok(current)
}

pub fn detect_project_id() -> Result<Uuid> {
    let project_root = find_project_root()?;
    let mr_pid_path = project_root.join(".mr_pid");
    
    if mr_pid_path.exists() {
        let content = fs::read_to_string(&mr_pid_path)?;
        let file = ProjectIdFile::parse(&content)?;
        Ok(file.project_id)
    } else {
        let file = ProjectIdFile::new(None);
        fs::write(&mr_pid_path, file.to_string())?;
        Ok(file.project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_project_id_file_creation() {
        let file = ProjectIdFile::new(Some("test-project".to_string()));
        assert!(!file.project_id.is_nil());
        assert_eq!(file.project_name, Some("test-project".to_string()));
    }
    
    #[test]
    fn test_project_id_file_parse() {
        let original = ProjectIdFile::new(None);
        let content = original.to_string();
        let parsed = ProjectIdFile::parse(&content).unwrap();
        
        assert_eq!(original.project_id, parsed.project_id);
    }
    
    #[test]
    fn test_detect_project_id_new_file() {
        let dir = tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        
        let id = detect_project_id().unwrap();
        assert!(!id.is_nil());
        
        let mr_pid_path = dir.path().join(".mr_pid");
        assert!(mr_pid_path.exists());
    }
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test --package memrecd project::detect`
Expected: 3 tests PASS

- [ ] **Step 4: 更新 memrecd/src/lib.rs 导出 project 模块**

```rust
// memrecd/src/lib.rs
pub mod storage;
pub mod server;
pub mod daemon;
pub mod importance;
pub mod lifecycle;
pub mod project;  // 新增
```

- [ ] **Step 5: Commit**

```bash
git add memrecd/src/project/mod.rs memrecd/src/project/detect.rs memrecd/src/lib.rs
git commit -m "feat: add project detection module with .mr_pid support"
```

---

## Phase 6.2: Memory结构变更 + 分块关联

### Task 2: Memory结构添加分块字段

**Files:**
- Modify: `common/src/types/memory.rs`

- [ ] **Step 1: 添加分块字段到 Memory 结构**

```rust
// common/src/types/memory.rs (修改 Memory 结构)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub memory_type: MemoryType,
    pub content: String,
    pub summary: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub importance: f32,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub project_id: Option<Uuid>,
    pub is_deleted: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    
    // 新增：分块关联字段
    pub chunk_group_id: Option<Uuid>,   // 同一原始文本的分块共享此ID
    pub chunk_index: Option<u32>,       // 分块序号 (0-based)
    pub chunk_total: Option<u32>,       // 总分块数
}
```

- [ ] **Step 2: 更新 Memory::new 初始化新字段**

```rust
// common/src/types/memory.rs (修改 Memory::new)
impl Memory {
    pub fn new(content: String, memory_type: MemoryType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            memory_type,
            content,
            summary: None,
            embedding: None,
            importance: 0.8,
            created_at: now,
            last_accessed: now,
            access_count: 0,
            tags: Vec::new(),
            metadata: HashMap::new(),
            project_id: None,
            is_deleted: false,
            deleted_at: None,
            
            // 新增字段初始化
            chunk_group_id: None,
            chunk_index: None,
            chunk_total: None,
        }
    }
    
    // 新增：设置分块信息
    pub fn with_chunk_info(mut self, group_id: Uuid, index: u32, total: u32) -> Self {
        self.chunk_group_id = Some(group_id);
        self.chunk_index = Some(index);
        self.chunk_total = Some(total);
        self
    }
    
    // 新增：检查是否为分块记忆
    pub fn is_chunked(&self) -> bool {
        self.chunk_group_id.is_some()
    }
}
```

- [ ] **Step 3: 添加测试**

```rust
// common/src/types/memory.rs (添加到 tests 模块)
#[test]
fn test_memory_chunk_fields() {
    let group_id = Uuid::new_v4();
    let memory = Memory::new("test".to_string(), MemoryType::Knowledge)
        .with_chunk_info(group_id, 0, 3);
    
    assert_eq!(memory.chunk_group_id, Some(group_id));
    assert_eq!(memory.chunk_index, Some(0));
    assert_eq!(memory.chunk_total, Some(3));
    assert!(memory.is_chunked());
}

#[test]
fn test_memory_chunk_serde() {
    let group_id = Uuid::new_v4();
    let memory = Memory::new("test".to_string(), MemoryType::Knowledge)
        .with_chunk_info(group_id, 1, 5);
    
    let json = serde_json::to_string(&memory).unwrap();
    let parsed: Memory = serde_json::from_str(&json).unwrap();
    
    assert_eq!(memory.chunk_group_id, parsed.chunk_group_id);
    assert_eq!(memory.chunk_index, parsed.chunk_index);
}
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test --package memrec-common types::memory`
Expected: 所有测试 PASS

- [ ] **Step 5: Commit**

```bash
git add common/src/types/memory.rs
git commit -m "feat: add chunk association fields to Memory struct"
```

---

## Phase 6.3: Qdrant集成 + VectorStore重写

### Task 3: 添加 Qdrant 依赖

**Files:**
- Modify: `Cargo.toml` (workspace)

- [ ] **Step 1: 更新 workspace Cargo.toml**

```toml
# Cargo.toml (workspace根目录)
[workspace.dependencies]
qdrant-client = "1.7"
fastembed = "3"

[workspace.package.memrecd.dependencies]
qdrant-client = { workspace = true }
fastembed = { workspace = true }
```

- [ ] **Step 2: 更新 memrecd/Cargo.toml**

```toml
# memrecd/Cargo.toml (添加依赖)
[dependencies]
qdrant-client = { workspace = true }
fastembed = { workspace = true }
```

- [ ] **Step 3: 验证依赖可用**

Run: `cargo check --package memrecd`
Expected: 无编译错误

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml memrecd/Cargo.toml
git commit -m "chore: add qdrant-client and fastembed dependencies"
```

### Task 4: 实现 QdrantVectorStore

**Files:**
- Create: `memrecd/src/storage/qdrant.rs`
- Modify: `memrecd/src/storage/mod.rs`
- Modify: `memrecd/src/storage/traits.rs`

- [ ] **Step 1: 更新 VectorStorage trait**

```rust
// memrecd/src/storage/traits.rs
use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;
use memrec_common::MemoryType;

#[async_trait]
pub trait VectorStorage: Send + Sync {
    async fn add(&self, id: &Uuid, embedding: &[f32], payload: VectorPayload) -> Result<()>;
    async fn remove(&self, id: &Uuid) -> Result<bool>;
    async fn search(&self, query: &[f32], filter: SearchFilter, top_k: usize) -> Result<Vec<SearchHit>>;
    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>>;
    async fn count(&self) -> Result<usize>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VectorPayload {
    pub project_id: Option<Uuid>,
    pub memory_type: MemoryType,
    pub tags: Vec<String>,
    pub content_preview: String,
    pub importance: f32,
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    pub project_id: Option<Uuid>,
    pub include_global: bool,
    pub memory_type: Option<MemoryType>,
    pub min_score: f32,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub memory_id: Uuid,
    pub score: f32,
    pub payload: VectorPayload,
}
```

- [ ] **Step 2: 创建 QdrantVectorStore**

```rust
// memrecd/src/storage/qdrant.rs
use anyhow::Result;
use async_trait::async_trait;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{CreateCollection, SearchPoints, PointStruct, Filter, Condition, FieldCondition, MatchValue};
use uuid::Uuid;
use std::path::PathBuf;
use super::traits::{VectorStorage, VectorPayload, SearchFilter, SearchHit};

pub struct QdrantVectorStore {
    client: Qdrant,
    collection_name: String,
    dimension: usize,
}

impl QdrantVectorStore {
    pub fn new(data_path: PathBuf, dimension: usize) -> Result<Self> {
        let qdrant_path = data_path.join("qdrant");
        let client = Qdrant::from_url(&format!("file://{}", qdrant_path.display()))
            .build()?;
        
        let collection_name = "memories".to_string();
        
        Ok(Self {
            client,
            collection_name,
            dimension,
        })
    }
    
    pub async fn ensure_collection(&self) -> Result<()> {
        let collections = self.client.list_collections().await?;
        
        if !collections.collections.iter().any(|c| c.name == self.collection_name) {
            self.client.create_collection(&CreateCollection {
                collection_name: self.collection_name.clone(),
                vectors_config: Some(qdrant_client::qdrant::VectorsConfig {
                    config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                        qdrant_client::qdrant::VectorParams {
                            size: self.dimension as u64,
                            distance: qdrant_client::qdrant::Distance::Cosine,
                            ..Default::default()
                        }
                    ))
                }),
                ..Default::default()
            }).await?;
        }
        
        Ok(())
    }
}

#[async_trait]
impl VectorStorage for QdrantVectorStore {
    async fn add(&self, id: &Uuid, embedding: &[f32], payload: VectorPayload) -> Result<()> {
        let point = PointStruct {
            id: Some(qdrant_client::qdrant::point_id::PointId::Uuid(id.to_string())),
            vectors: Some(qdrant_client::qdrant::Vectors::Vector(embedding.to_vec())),
            payload: Some(serde_json::to_value(payload)?),
        };
        
        self.client.upsert_points_blocking(self.collection_name.clone(), vec![point], None).await?;
        Ok(())
    }
    
    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let result = self.client.delete_points_blocking(
            self.collection_name.clone(),
            &qdrant_client::qdrant::Filter {
                must: vec![Condition::Field(FieldCondition {
                    key: "id".to_string(),
                    match: Some(MatchValue::Uuid(id.to_string())),
                    ..Default::default()
                })],
                ..Default::default()
            },
            None
        ).await?;
        
        Ok(result.result.unwrap_or(false))
    }
    
    async fn search(&self, query: &[f32], filter: SearchFilter, top_k: usize) -> Result<Vec<SearchHit>> {
        let mut conditions = vec![];
        
        if let Some(project_id) = filter.project_id {
            conditions.push(Condition::Field(FieldCondition {
                key: "project_id".to_string(),
                match: Some(MatchValue::Uuid(project_id.to_string())),
                ..Default::default()
            }));
        }
        
        if filter.include_global {
            conditions.push(Condition::Field(FieldCondition {
                key: "project_id".to_string(),
                match: Some(MatchValue::Uuid(Uuid::nil().to_string())),
                ..Default::default()
            }));
        }
        
        let qdrant_filter = if conditions.is_empty() {
            None
        } else {
            Some(Filter {
                should: conditions,
                ..Default::default()
            })
        };
        
        let result = self.client.search_points(SearchPoints {
            collection_name: self.collection_name.clone(),
            vector: query.to_vec(),
            filter: qdrant_filter,
            limit: top_k as u64,
            with_payload: Some(true.into()),
            ..Default::default()
        }).await?;
        
        let hits = result.result.into_iter()
            .filter(|r| r.score >= filter.min_score)
            .map(|r| {
                let payload: VectorPayload = serde_json::from_value(r.payload.unwrap_or_default()).unwrap_or_default();
                SearchHit {
                    memory_id: Uuid::parse_str(&r.id.unwrap_or_default()).unwrap_or_default(),
                    score: r.score,
                    payload,
                }
            })
            .collect();
        
        Ok(hits)
    }
    
    async fn get(&self, id: &Uuid) -> Result<Option<Vec<f32>>> {
        let result = self.client.get_points(
            self.collection_name.clone(),
            &vec![qdrant_client::qdrant::point_id::PointId::Uuid(id.to_string())],
            Some(true.into()),
            None,
            None
        ).await?;
        
        if let Some(point) = result.result.first() {
            if let Some(qdrant_client::qdrant::Vectors::Vector(v)) = &point.vectors {
                return Ok(Some(v.to_vec()));
            }
        }
        
        Ok(None)
    }
    
    async fn count(&self) -> Result<usize> {
        let result = self.client.count_points(
            self.collection_name.clone(),
            None,
            true
        ).await?;
        
        Ok(result.result.count as usize)
    }
}

impl Default for VectorPayload {
    fn default() -> Self {
        Self {
            project_id: None,
            memory_type: MemoryType::Conversation,
            tags: Vec::new(),
            content_preview: String::new(),
            importance: 0.5,
            chunk_group_id: None,
            chunk_index: None,
            chunk_total: None,
        }
    }
}
```

- [ ] **Step 3: 更新 storage/mod.rs 导出**

```rust
// memrecd/src/storage/mod.rs
pub mod traits;
pub mod rocksdb;
pub mod memory_store;
pub mod vector_store;
pub mod qdrant;  // 新增

pub use traits::{MemoryStorage, VectorStorage, VectorPayload, SearchFilter, SearchHit};
pub use memory_store::MemoryStore;
pub use vector_store::VectorStore;
pub use qdrant::QdrantVectorStore;  // 新增
```

- [ ] **Step 4: 运行编译检查**

Run: `cargo check --package memrecd`
Expected: 无编译错误（可能有警告）

- [ ] **Step 5: Commit**

```bash
git add memrecd/src/storage/qdrant.rs memrecd/src/storage/traits.rs memrecd/src/storage/mod.rs
git commit -m "feat: implement QdrantVectorStore with embedded storage"
```

---

## Phase 6.4: Embedding生成 + 语义检索

### Task 5: 实现 FastEmbed 生成器

**Files:**
- Create: `memrecd/src/embedding/mod.rs`
- Create: `memrecd/src/embedding/fastembed.rs`

- [ ] **Step 1: 创建 embedding 模块入口**

```rust
// memrecd/src/embedding/mod.rs
pub mod fastembed;

pub use fastembed::FastEmbedGenerator;
pub use fastembed::EmbeddingModel;
```

- [ ] **Step 2: 实现 FastEmbed 生成器**

```rust
// memrecd/src/embedding/fastembed.rs
use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct FastEmbedGenerator {
    model: TextEmbedding,
    dimension: usize,
}

impl FastEmbedGenerator {
    pub fn new() -> Result<Self> {
        let model = TextEmbedding::try_new(InitOptions {
            model_name: EmbeddingModel::AllMiniLML6V2,
            ..Default::default()
        })?;
        
        Ok(Self {
            model,
            dimension: 384, // all-MiniLM-L6-v2 的维度
        })
    }
    
    pub fn dimension(&self) -> usize {
        self.dimension
    }
    
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.model.embed(vec![text], None)?;
        
        if let Some(embedding) = embeddings.first() {
            Ok(embedding.iter().map(|f| f as f32).collect())
        } else {
            anyhow::bail!("Failed to generate embedding")
        }
    }
    
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let embeddings = self.model.embed(texts.to_vec(), None)?;
        
        Ok(embeddings.into_iter()
            .map(|e| e.iter().map(|f| f as f32).collect())
            .collect())
    }
}

impl Default for FastEmbedGenerator {
    fn default() -> Self {
        Self::new().expect("Failed to initialize FastEmbed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_embedding_dimension() {
        let generator = FastEmbedGenerator::new().unwrap();
        assert_eq!(generator.dimension(), 384);
    }
    
    #[test]
    fn test_single_embedding() {
        let generator = FastEmbedGenerator::new().unwrap();
        let embedding = generator.embed("test text").unwrap();
        
        assert_eq!(embedding.len(), 384);
    }
}
```

- [ ] **Step 3: 更新 memrecd/src/lib.rs 导出**

```rust
// memrecd/src/lib.rs (添加 embedding 模块)
pub mod storage;
pub mod server;
pub mod daemon;
pub mod importance;
pub mod lifecycle;
pub mod project;
pub mod embedding;  // 新增
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test --package memrecd embedding::fastembed`
Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```bash
git add memrecd/src/embedding/mod.rs memrecd/src/embedding/fastembed.rs memrecd/src/lib.rs
git commit -m "feat: implement FastEmbed generator for local embedding"
```

### Task 6: 实现语义检索 Handler

**Files:**
- Modify: `common/src/protocol/request.rs`
- Modify: `common/src/protocol/response.rs`
- Modify: `memrecd/src/server/handler.rs`

- [ ] **Step 1: 添加 SearchMemory Action**

```rust
// common/src/protocol/request.rs (添加到 RequestAction enum)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RequestAction {
    Add,
    Get,
    Update,
    Delete,
    Search,       // 现有
    List,
    Tag,
    
    SearchMemory, // 新增：语义检索
    GetProjectInfo, // 新增：项目信息
    MergeChunks,    // 新增：合并分块
    
    ProjectCreate,
    ProjectList,
    ProjectSwitch,
    ProjectDelete,
    
    ConfigGet,
    ConfigSet,
    
    Stats,
}

// 添加新的参数类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestParams {
    // ... 现有类型 ...
    SearchMemory(SearchMemoryParams),  // 新增
    GetProjectInfo(GetProjectInfoParams), // 新增
    MergeChunks(MergeChunksParams),     // 新增
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryParams {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default = "default_include_global")]
    pub include_global: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_min_score")]
    pub min_score: f32,
}

fn default_include_global() -> bool { true }
fn default_min_score() -> f32 { 0.7 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetProjectInfoParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeChunksParams {
    pub chunk_group_id: Uuid,
}
```

- [ ] **Step 2: 添加 SearchResult Response**

```rust
// common/src/protocol/response.rs (添加新的结果类型)
use uuid::Uuid;
use crate::types::MemoryType;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryResult {
    pub results: Vec<SearchHit>,
    pub total: usize,
    pub query_embedding_time_ms: u64,
    pub search_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub memory_id: Uuid,
    pub score: f32,
    pub memory_type: MemoryType,
    pub content_preview: String,
    pub project_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub is_chunked: bool,
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfoResult {
    pub project_id: Uuid,
    pub project_name: Option<String>,
    pub project_root: String,
    pub memory_count: usize,
    pub mr_pid_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeChunksResult {
    pub memory_id: Uuid,
    pub merged_content: String,
    pub original_ids: Vec<Uuid>,
    pub chunk_count: usize,
}

// 更新 ResponseResult enum
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseResult {
    Memory(MemoryResult),
    MemoryList(MemoryListResult),
    Stats(StatsResult),
    Success(SuccessResult),
    
    SearchMemory(SearchMemoryResult),  // 新增
    ProjectInfo(ProjectInfoResult),     // 新增
    MergeChunks(MergeChunksResult),     // 新增
}
```

- [ ] **Step 3: 实现语义检索 Handler**

```rust
// memrecd/src/server/handler.rs (添加新的处理方法)
use crate::embedding::FastEmbedGenerator;
use crate::project::detect_project_id;
use crate::storage::{VectorStorage, SearchFilter};

pub struct Router {
    storage: Arc<dyn MemoryStorage>,
    vector_store: Arc<dyn VectorStorage>,
    embedder: Arc<FastEmbedGenerator>,
}

impl Router {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        vector_store: Arc<dyn VectorStorage>,
        embedder: Arc<FastEmbedGenerator>,
    ) -> Self {
        Self { storage, vector_store, embedder }
    }
    
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method {
            RequestAction::Add => self.handle_add(request.params, request.id).await,
            RequestAction::Get => self.handle_get(request.params, request.id).await,
            RequestAction::List => self.handle_list(request.params, request.id).await,
            RequestAction::Delete => self.handle_delete(request.params, request.id).await,
            RequestAction::Stats => self.handle_stats(request.id).await,
            
            // 新增处理
            RequestAction::SearchMemory => self.handle_search_memory(request.params, request.id).await,
            RequestAction::GetProjectInfo => self.handle_project_info(request.id).await,
            RequestAction::MergeChunks => self.handle_merge_chunks(request.params, request.id).await,
            
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32601,
                    message: "Method not found".to_string(),
                    data: None,
                },
                request.id
            )
        }
    }
    
    async fn handle_search_memory(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SearchMemory(p)) => {
                let start = std::time::Instant::now();
                
                // 生成 query embedding
                let embedding = match self.embedder.embed(&p.query) {
                    Ok(e) => e,
                    Err(e) => return JsonRpcResponse::error(
                        JsonRpcError { code: -32002, message: e.to_string(), data: None },
                        id
                    ),
                };
                let embed_time = start.elapsed().as_millis() as u64;
                
                // 构建搜索过滤器
                let filter = SearchFilter {
                    project_id: p.project_id,
                    include_global: p.include_global,
                    memory_type: p.memory_type,
                    min_score: p.min_score,
                };
                
                // 执行向量搜索
                let search_start = std::time::Instant::now();
                let hits = match self.vector_store.search(&embedding, filter, p.top_k).await {
                    Ok(h) => h,
                    Err(e) => return JsonRpcResponse::error(
                        JsonRpcError { code: -32003, message: e.to_string(), data: None },
                        id
                    ),
                };
                let search_time = search_start.elapsed().as_millis() as u64;
                
                // 转换为 SearchHit
                let results: Vec<memrec_common::SearchHit> = hits.into_iter()
                    .map(|h| {
                        let memory = self.storage.get(&h.memory_id).await.ok().flatten();
                        memrec_common::SearchHit {
                            memory_id: h.memory_id,
                            score: h.score,
                            memory_type: h.payload.memory_type,
                            content_preview: h.payload.content_preview,
                            project_id: h.payload.project_id,
                            tags: h.payload.tags,
                            is_chunked: h.payload.chunk_group_id.is_some(),
                            chunk_group_id: h.payload.chunk_group_id,
                            chunk_index: h.payload.chunk_index,
                            chunk_total: h.payload.chunk_total,
                            created_at: memory.map(|m| m.created_at).unwrap_or_default(),
                        }
                    })
                    .collect();
                
                JsonRpcResponse::success(
                    ResponseResult::SearchMemory(SearchMemoryResult {
                        results,
                        total: results.len(),
                        query_embedding_time_ms: embed_time,
                        search_time_ms: search_time,
                    }),
                    id
                )
            }
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params".to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_project_info(&self, id: u64) -> JsonRpcResponse {
        match detect_project_id() {
            Ok(project_id) => {
                let project_root = crate::project::find_project_root()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                
                let mr_pid_exists = std::path::Path::new(&project_root)
                    .join(".mr_pid")
                    .exists();
                
                let memory_count = self.storage.list_by_project(&project_id).await
                    .unwrap_or_default()
                    .len();
                
                JsonRpcResponse::success(
                    ResponseResult::ProjectInfo(ProjectInfoResult {
                        project_id,
                        project_name: None,
                        project_root,
                        memory_count,
                        mr_pid_exists,
                    }),
                    id
                )
            }
            Err(e) => JsonRpcResponse::error(
                JsonRpcError { code: -32004, message: e.to_string(), data: None },
                id
            )
        }
    }
    
    async fn handle_merge_chunks(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::MergeChunks(p)) => {
                match self.storage.get_chunks_by_group(&p.chunk_group_id).await {
                    Ok(chunks) => {
                        if chunks.is_empty() {
                            return JsonRpcResponse::error(
                                JsonRpcError { code: -32005, message: "No chunks found".to_string(), data: None },
                                id
                            );
                        }
                        
                        // 按 chunk_index 排序
                        let mut sorted_chunks = chunks;
                        sorted_chunks.sort_by_key(|c| c.chunk_index.unwrap_or(0));
                        
                        let merged_content = sorted_chunks.iter()
                            .map(|c| c.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        
                        let original_ids = sorted_chunks.iter()
                            .map(|c| c.id)
                            .collect();
                        
                        JsonRpcResponse::success(
                            ResponseResult::MergeChunks(MergeChunksResult {
                                memory_id: p.chunk_group_id,
                                merged_content,
                                original_ids,
                                chunk_count: sorted_chunks.len(),
                            }),
                            id
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError { code: -32000, message: e.to_string(), data: None },
                        id
                    )
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError { code: -32602, message: "Invalid params".to_string(), data: None },
                id
            )
        }
    }
}
```

- [ ] **Step 4: 更新 MemoryStorage trait**

```rust
// memrecd/src/storage/traits.rs (添加新方法)
#[async_trait]
pub trait MemoryStorage: Send + Sync {
    async fn save(&self, memory: &Memory) -> Result<()>;
    async fn get(&self, id: &Uuid) -> Result<Option<Memory>>;
    async fn list(&self, limit: usize) -> Result<Vec<Memory>>;
    async fn delete(&self, id: &Uuid) -> Result<bool>;
    async fn count(&self) -> Result<usize>;
    async fn count_deleted(&self) -> Result<usize>;
    
    // 新增方法
    async fn list_by_project(&self, project_id: &Uuid) -> Result<Vec<Memory>>;
    async fn get_chunks_by_group(&self, chunk_group_id: &Uuid) -> Result<Vec<Memory>>;
    async fn save_with_embedding(&self, memory: &Memory, embedding: &[f32]) -> Result<()>;
}
```

- [ ] **Step 5: 实现 MemoryStore 新方法**

```rust
// memrecd/src/storage/memory_store.rs (添加新方法实现)
#[async_trait]
impl MemoryStorage for MemoryStore {
    // ... 现有方法 ...
    
    async fn list_by_project(&self, project_id: &Uuid) -> Result<Vec<Memory>> {
        let cf = self.get_cf("memory")?;
        let iter = self.db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
        
        let memories: Vec<Memory> = iter
            .filter_map(|item| {
                let (_, value) = item.ok()?;
                let memory: Memory = serde_json::from_slice(&value).ok()?;
                if memory.project_id == Some(*project_id) && !memory.is_deleted {
                    Some(memory)
                } else {
                    None
                }
            })
            .collect();
        
        Ok(memories)
    }
    
    async fn get_chunks_by_group(&self, chunk_group_id: &Uuid) -> Result<Vec<Memory>> {
        let cf = self.get_cf("memory")?;
        let iter = self.db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
        
        let chunks: Vec<Memory> = iter
            .filter_map(|item| {
                let (_, value) = item.ok()?;
                let memory: Memory = serde_json::from_slice(&value).ok()?;
                if memory.chunk_group_id == Some(*chunk_group_id) && !memory.is_deleted {
                    Some(memory)
                } else {
                    None
                }
            })
            .collect();
        
        Ok(chunks)
    }
    
    async fn save_with_embedding(&self, memory: &Memory, embedding: &[f32]) -> Result<()> {
        let mut memory = memory.clone();
        memory.embedding = Some(embedding.to_vec());
        self.save(&memory).await
    }
}
```

- [ ] **Step 6: 运行编译检查**

Run: `cargo check --package memrecd`
Expected: 无编译错误

- [ ] **Step 7: Commit**

```bash
git add common/src/protocol/request.rs common/src/protocol/response.rs memrecd/src/server/handler.rs memrecd/src/storage/traits.rs memrecd/src/storage/memory_store.rs
git commit -m "feat: implement semantic search handler with FastEmbed"
```

---

## Phase 6.5: CLI命令简化 + JSON输出

### Task 7: 实现 search CLI 命令

**Files:**
- Create: `memrec/src/commands/search.rs`
- Modify: `memrec/src/commands/mod.rs`
- Modify: `memrec/src/main.rs`

- [ ] **Step 1: 创建 search 命令文件**

```rust
// memrec/src/commands/search.rs
use clap::Args;
use uuid::Uuid;
use memrec_common::{
    JsonRpcRequest, RequestAction, RequestParams,
    SearchMemoryParams, MemoryType,
};
use crate::client::MemrecClient;

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Search query text
    #[arg(short, long)]
    query: String,
    
    /// Number of results to return
    #[arg(short = 'k', long, default_value = "10")]
    top_k: usize,
    
    /// Minimum similarity score
    #[arg(long, default_value = "0.7")]
    min_score: f32,
    
    /// Search only current project memories
    #[arg(long)]
    project_only: bool,
    
    /// Search only global memories
    #[arg(long)]
    global_only: bool,
    
    /// Filter by memory type
    #[arg(long)]
    mtype: Option<String>,
    
    /// Output in human-readable format
    #[arg(long)]
    human: bool,
}

pub async fn execute(client: &MemrecClient, args: SearchArgs) -> anyhow::Result<()> {
    let memory_type = args.mtype.and_then(|t| match t.to_lowercase().as_str() {
        "decision" => Some(MemoryType::Decision),
        "knowledge" => Some(MemoryType::Knowledge),
        "context" => Some(MemoryType::Context),
        "preference" => Some(MemoryType::Preference),
        "conversation" => Some(MemoryType::Conversation),
        _ => None,
    });
    
    let include_global = !args.project_only;
    let project_id = if args.global_only {
        Some(Uuid::nil())
    } else {
        None
    };
    
    let request = JsonRpcRequest::new(
        RequestAction::SearchMemory,
        Some(RequestParams::SearchMemory(SearchMemoryParams {
            query: args.query,
            project_id,
            include_global,
            memory_type,
            top_k: args.top_k,
            min_score: args.min_score,
        })),
        1,
    );
    
    let response = client.send_request(&request)?;
    
    if args.human {
        print_human_output(&response);
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }
    
    Ok(())
}

fn print_human_output(response: &memrec_common::JsonRpcResponse) {
    use memrec_common::{ResponseResult, SearchMemoryResult};
    
    if let Some(memrec_common::ResponseResult::SearchMemory(result)) = response.result {
        println!("Found {} memories (score >= {:.1}):\n", result.total, 0.7);
        
        for hit in &result.results {
            println!("[{}] {} (score: {:.2})",
                hit.memory_type.to_string().to_uppercase(),
                truncate(&hit.content_preview, 50),
                hit.score
            );
            println!("  ID: {}", hit.memory_id);
            if let Some(pid) = hit.project_id {
                if pid.is_nil() {
                    println!("  Project: (global)");
                } else {
                    println!("  Project: {}", pid);
                }
            }
            println!("  Tags: {:?}", hit.tags);
            println!("  Created: {}", hit.created_at.format("%Y-%m-%d"));
            
            if hit.is_chunked {
                println!("  ⚠️  Chunked memory ({}/{}). Use `memrec get {} --merge`.",
                    hit.chunk_index.unwrap_or(0) + 1,
                    hit.chunk_total.unwrap_or(0),
                    hit.chunk_group_id.unwrap_or_default()
                );
            }
            println!();
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        s.chars().take(max_len).collect::<String>() + "..."
    } else {
        s.to_string()
    }
}
```

- [ ] **Step 2: 更新 commands/mod.rs**

```rust
// memrec/src/commands/mod.rs
pub mod memory;
pub mod search;  // 新增

pub use memory::{AddArgs, GetArgs, ListArgs, DeleteArgs, StatsArgs};
pub use search::{SearchArgs, execute as search_execute};  // 新增
```

- [ ] **Step 3: 更新 main.rs 添加 search 子命令**

```rust
// memrec/src/main.rs
use clap::{Parser, Subcommand};
use memrec::commands::{AddArgs, GetArgs, ListArgs, DeleteArgs, StatsArgs, SearchArgs};

#[derive(Parser)]
#[command(name = "memrec")]
#[command(about = "AI memory persistence system", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Add(AddArgs),
    Get(GetArgs),
    List(ListArgs),
    Delete(DeleteArgs),
    Stats(StatsArgs),
    Search(SearchArgs),  // 新增
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = memrec::client::MemrecClient::new()?;
    
    match cli.command {
        Commands::Add(args) => memrec::commands::memory::execute_add(&client, args).await?,
        Commands::Get(args) => memrec::commands::memory::execute_get(&client, args).await?,
        Commands::List(args) => memrec::commands::memory::execute_list(&client, args).await?,
        Commands::Delete(args) => memrec::commands::memory::execute_delete(&client, args).await?,
        Commands::Stats(args) => memrec::commands::memory::execute_stats(&client, args).await?,
        Commands::Search(args) => memrec::commands::search::execute(&client, args).await?,  // 新增
    }
    
    Ok(())
}
```

- [ ] **Step 4: 运行编译检查**

Run: `cargo check --package memrec`
Expected: 无编译错误

- [ ] **Step 5: Commit**

```bash
git add memrec/src/commands/search.rs memrec/src/commands/mod.rs memrec/src/main.rs
git commit -m "feat: add semantic search CLI command with JSON output"
```

### Task 8: 更新现有命令添加 --global 参数

**Files:**
- Modify: `memrec/src/commands/memory.rs`

- [ ] **Step 1: 更新 AddArgs 添加 --global**

```rust
// memrec/src/commands/memory.rs (修改 AddArgs)
#[derive(Args, Debug)]
pub struct AddArgs {
    /// Memory content
    #[arg(short, long)]
    content: String,
    
    /// Memory type
    #[arg(short, long, default_value = "conversation")]
    mtype: String,
    
    /// Tags for the memory
    #[arg(short, long)]
    tag: Vec<String>,
    
    /// Mark as global memory (shared across projects)
    #[arg(long)]
    global: bool,  // 新增
    
    /// Output in human-readable format
    #[arg(long)]
    human: bool,  // 新增
}

pub async fn execute_add(client: &MemrecClient, args: AddArgs) -> anyhow::Result<()> {
    let memory_type = parse_memory_type(&args.mtype)?;
    
    let project_id = if args.global {
        Some(Uuid::nil())
    } else {
        // 自动检测项目ID
        memrecd::project::detect_project_id().ok()
    };
    
    // ... 其余实现
}
```

- [ ] **Step 2: 更新 ListArgs 添加范围参数**

```rust
// memrec/src/commands/memory.rs (修改 ListArgs)
#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number of memories to list
    #[arg(short, long, default_value = "20")]
    limit: usize,
    
    /// List only current project memories
    #[arg(long)]
    project_only: bool,  // 新增
    
    /// List only global memories
    #[arg(long)]
    global_only: bool,  // 新增
    
    /// Output in human-readable format
    #[arg(long)]
    human: bool,  // 新增
}
```

- [ ] **Step 3: 更新 GetArgs 添加 --merge**

```rust
// memrec/src/commands/memory.rs (修改 GetArgs)
#[derive(Args, Debug)]
pub struct GetArgs {
    /// Memory ID
    #[arg(short, long)]
    id: String,
    
    /// Merge chunked memories
    #[arg(long)]
    merge: bool,  // 新增
    
    /// Output in human-readable format
    #[arg(long)]
    human: bool,  // 新增
}

pub async fn execute_get(client: &MemrecClient, args: GetArgs) -> anyhow::Result<()> {
    let id = Uuid::parse_str(&args.id)?;
    
    if args.merge {
        // 调用 MergeChunks
        let request = JsonRpcRequest::new(
            RequestAction::MergeChunks,
            Some(RequestParams::MergeChunks(MergeChunksParams {
                chunk_group_id: id,
            })),
            1,
        );
        
        let response = client.send_request(&request)?;
        
        if args.human {
            print_merge_human_output(&response);
        } else {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
    } else {
        // 正常 get
        // ...
    }
    
    Ok(())
}
```

- [ ] **Step 4: 运行编译检查**

Run: `cargo check --package memrec`
Expected: 无编译错误

- [ ] **Step 5: Commit**

```bash
git add memrec/src/commands/memory.rs
git commit -m "feat: add --global, --project-only, --merge params to CLI commands"
```

---

## Phase 6.6: Skill更新 + 文档

### Task 9: 创建 Phase 6 Skill 文档

**Files:**
- Create: `docs/skills/memrec-skill-phase6.md`

- [ ] **Step 1: 创建完整 Skill 文档**

```markdown
# MemRec - AI记忆持久化系统 (Phase 6)

为AI CLI工具提供跨会话记忆能力，支持项目隔离和语义检索。

## 核心原则

**MemRec是面向AI Agent的工具，而非人类用户。**
- 默认JSON输出，便于AI解析
- 自动项目检测，无需手动管理
- 语义检索优先，标签过滤次要

## 触发场景

1. 重要决策需记录 → `memrec add --mtype decision --tag critical`
2. 关键知识需保存 → `memrec add --mtype knowledge`
3. 项目上下文需跨会话保持 → 自动关联项目ID
4. 用户偏好需记忆 → `memrec add --mtype preference --global`
5. 检索历史知识辅助当前任务 → `memrec search "关键词"`

## 核心命令

### 存储记忆

```bash
memrec add "内容" --mtype <type> [--tag <tag>] [--global]
```

记忆类型：
- `decision` - 关键决策（推荐--tag critical）
- `knowledge` - 知识点/最佳实践
- `context` - 项目配置/环境信息
- `preference` - 用户偏好（推荐--global）

返回JSON：
```json
{"memory_id": "550e8400-...", "project_id": "...", "chunk_count": 1}
```

### 语义检索

```bash
memrec search "关键词" [--mtype <type>] [--project-only] [--global-only] [-k <num>]
```

返回JSON：
```json
{
  "results": [
    {"memory_id": "...", "score": 0.92, "content_preview": "...", "tags": [...]}
  ],
  "total": 3,
  "search_time_ms": 15
}
```

参数：
- `--project-only` 仅当前项目
- `--global-only` 仅公共记忆
- `-k 20` 返回20条
- `--min-score 0.8` 最低相似度

### 获取详情

```bash
memrec get <memory_id> [--merge]
```

`--merge` 用于合并分块记忆，返回完整内容。

### 项目信息

```bash
memrec project
```

返回JSON：
```json
{
  "project_id": "550e8400-...",
  "project_root": "/path/to/project",
  "memory_count": 25,
  "mr_pid_exists": true
}
```

### 统计信息

```bash
memrec stats
```

## 工作流集成

**开始任务时：**
```bash
memrec search "相关历史" --project-only
memrec project
```

**做出决策后：**
```bash
memrec add "选择XXX方案" --mtype decision --tag critical
```

**用户表达偏好后：**
```bash
memrec add "用户偏好YYY" --mtype preference --global
```

## 项目隔离

- **公共记忆**：`--global` 标记，存储在 `project_id=nil`
- **项目记忆**：自动检测项目ID，存储在 `.mr_pid` 文件中
- **检索范围**：默认项目+公共，可通过参数限制

## 长文本处理

超过7.5KB自动拆分：
- 每块独立embedding
- 共享 `chunk_group_id`
- 使用 `--merge` 获取完整内容

## 数据位置

```
~/.memrec/
├── memrecd.sock      # Unix Socket
├── db/               # RocksDB元数据
└── qdrant/           # Qdrant向量索引
```

## 安装

```bash
cargo build --release
install -m 755 target/release/memrecd ~/.local/bin/
install -m 755 target/release/memrec ~/.local/bin/
./scripts/start.sh
```

## 验证

```bash
memrec search "test"
memrec stats
```
```

- [ ] **Step 2: Commit**

```bash
git add docs/skills/memrec-skill-phase6.md
git commit -m "docs: create Phase 6 skill documentation"
```

### Task 10: 运行完整测试

- [ ] **Step 1: 运行所有测试**

Run: `cargo test --workspace`
Expected: 所有测试 PASS

- [ ] **Step 2: 手动验证功能**

```bash
# 启动守护进程
./scripts/start.sh

# 测试语义检索
memrec search "test"

# 测试项目检测
memrec project

# 测试添加公共记忆
memrec add "测试公共记忆" --mtype knowledge --global

# 验证JSON输出
memrec stats
```

- [ ] **Step 3: 最终 Commit**

```bash
git add -A
git commit -m "feat: complete Phase 6 - project isolation + semantic search"
```

---

## Self-Review Checklist

**1. Spec Coverage:**
- [ ] 公共/项目记忆分离 → Task 1, Task 8
- [ ] `.mr_pid` 文件管理 → Task 1
- [ ] 分块关联字段 → Task 2
- [ ] Qdrant嵌入式存储 → Task 3, Task 4
- [ ] FastEmbed embedding → Task 5
- [ ] 语义检索Handler → Task 6
- [ ] search CLI命令 → Task 7
- [ ] JSON默认输出 → Task 7, Task 8
- [ ] Skill文档更新 → Task 9

**2. Placeholder Scan:**
- [ ] 无 TBD/TODO
- [ ] 无 "implement later"
- [ ] 无 "add error handling" 泛泛描述
- [ ] 所有代码步骤有完整代码块

**3. Type Consistency:**
- [ ] `SearchMemoryParams` 在 request.rs 和 handler.rs 一致
- [ ] `SearchHit` 在 response.rs 和 handler.rs 一致
- [ ] `VectorPayload` 在 traits.rs 和 qdrant.rs 一致
- [ ] Memory新增字段在所有使用处同步

---

*Created: 2026-04-23*
*Total Tasks: 10*
*Estimated Time: ~5 days*