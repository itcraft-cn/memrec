# MemRec Phase 1: 基础设施实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 Rust 项目结构和基础类型定义，为后续存储层和服务层提供共享代码。

**Architecture:** Workspace 结构，包含 common、memrecd、memrec 三个 crate；common crate 定义核心数据类型和协议。

**Tech Stack:** Rust, serde, uuid, chrono, anyhow, thiserror

---

## 文件结构

### 新建文件

```
memrec/
├── Cargo.toml                   # Workspace定义
├── .gitignore                   # Git忽略配置
├── common/
│   ├── Cargo.toml               # common crate配置
│   └── src/
│       ├── lib.rs               # 模块导出
│       ├── types/
│       │   ├── mod.rs           # types模块导出
│       │   ├── memory.rs        # Memory类型定义
│       │   ├── project.rs       # Project类型定义
│       │   └── config.rs        # Config类型定义
│       ├── protocol/
│       │   ├── mod.rs           # protocol模块导出
│       │   ├── request.rs       # JSON-RPC请求类型
│       │   └── response.rs      # JSON-RPC响应类型
│       │   └── error.rs         # 错误类型定义
│       └── error.rs             # 公共错误类型
├── memrecd/
│   ├── Cargo.toml               # memrecd crate配置
│   └── src/
│       ├── main.rs              # 占位入口
├── memrec/
│   ├── Cargo.toml               # memrec crate配置
│   └── src/
│       ├── main.rs              # 占位入口
```

---

## Task 1: 创建 Workspace 项目结构

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`

- [ ] **Step 1: 创建 Workspace Cargo.toml**

```toml
[workspace]
members = ["common", "memrecd", "memrec"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["MemRec Team"]
license = "MIT"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
thiserror = "1"
```

- [ ] **Step 2: 创建 .gitignore**

```gitignore
# Build artifacts
/target/
Cargo.lock

# IDE
.idea/
.vscode/
*.swp
*.swo

# Data
/.memrec/
/data/

# Logs
*.log

# Test
/test_data/
```

- [ ] **Step 3: 初始化 Workspace**

```bash
cargo init --workspace
```

Expected: 创建基本项目结构

- [ ] **Step 4: 验证 Workspace 结构**

```bash
ls -la
```

Expected: 看到 `Cargo.toml` 和空的 `src/` 目录

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml .gitignore
git commit -m "feat: initialize workspace structure"
```

---

## Task 2: 创建 common crate 配置

**Files:**
- Create: `common/Cargo.toml`

- [ ] **Step 1: 初始化 common crate**

```bash
cargo new common --lib
```

Expected: 创建 `common/` 目录和基本文件

- [ ] **Step 2: 配置 common/Cargo.toml**

```toml
[package]
name = "memrec-common"
version.workspace = true
edition.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
anyhow.workspace = true
thiserror.workspace = true
```

- [ ] **Step 3: 验证 common crate**

```bash
cargo check -p memrec-common
```

Expected: PASS，无错误

- [ ] **Step 4: 提交**

```bash
git add common/
git commit -m "feat: add common crate configuration"
```

---

## Task 3: 定义 Memory 类型

**Files:**
- Create: `common/src/types/mod.rs`
- Create: `common/src/types/memory.rs`
- Create: `common/src/lib.rs`

- [ ] **Step 1: 创建 types 模块结构**

```bash
mkdir -p common/src/types
```

- [ ] **Step 2: 定义 MemoryType 枚举**

File: `common/src/types/memory.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Conversation,
    Knowledge,
    Decision,
    Preference,
    Context,
}

impl Default for MemoryType {
    fn default() -> Self {
        MemoryType::Conversation
    }
}
```

- [ ] **Step 3: 编写 MemoryType 测试**

File: `common/src/types/memory.rs` (追加)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_memory_type_serde() {
        let types = [
            MemoryType::Conversation,
            MemoryType::Knowledge,
            MemoryType::Decision,
            MemoryType::Preference,
            MemoryType::Context,
        ];
        
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let parsed: MemoryType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, parsed);
        }
    }
    
    #[test]
    fn test_memory_type_json_values() {
        assert_eq!(serde_json::to_string(&MemoryType::Conversation).unwrap(), "\"conversation\"");
        assert_eq!(serde_json::to_string(&MemoryType::Knowledge).unwrap(), "\"knowledge\"");
    }
}
```

- [ ] **Step 4: 运行测试验证**

```bash
cargo test -p memrec-common --lib types::memory::tests
```

Expected: 2 tests PASS

- [ ] **Step 5: 定义 Memory 结构体**

File: `common/src/types/memory.rs` (追加)

```rust
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

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
}

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
        }
    }
    
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
    
    pub fn with_project(mut self, project_id: Uuid) -> Self {
        self.project_id = Some(project_id);
        self
    }
    
    pub fn access(&mut self) {
        self.last_accessed = Utc::now();
        self.access_count += 1;
    }
}
```

- [ ] **Step 6: 编写 Memory 测试**

File: `common/src/types/memory.rs` (追加到 tests 模块)

```rust
    #[test]
    fn test_memory_creation() {
        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge);
        
        assert!(!memory.id.to_string().is_empty());
        assert_eq!(memory.memory_type, MemoryType::Knowledge);
        assert_eq!(memory.content, "test content");
        assert!(memory.embedding.is_none());
        assert_eq!(memory.importance, 0.8);
        assert_eq!(memory.access_count, 0);
        assert!(memory.tags.is_empty());
        assert!(!memory.is_deleted);
    }
    
    #[test]
    fn test_memory_with_tags() {
        let memory = Memory::new("test".to_string(), MemoryType::Decision)
            .with_tags(vec!["important".to_string(), "project-x".to_string()]);
        
        assert_eq!(memory.tags.len(), 2);
        assert!(memory.tags.contains(&"important".to_string()));
    }
    
    #[test]
    fn test_memory_access() {
        let mut memory = Memory::new("test".to_string(), MemoryType::Conversation);
        let initial_accessed = memory.last_accessed;
        
        memory.access();
        
        assert!(memory.last_accessed > initial_accessed);
        assert_eq!(memory.access_count, 1);
    }
    
    #[test]
    fn test_memory_serde() {
        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["tag1".to_string()]);
        
        let json = serde_json::to_string(&memory).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();
        
        assert_eq!(memory.id, parsed.id);
        assert_eq!(memory.content, parsed.content);
        assert_eq!(memory.tags, parsed.tags);
    }
```

- [ ] **Step 7: 运行所有 memory 测试**

```bash
cargo test -p memrec-common --lib types::memory
```

Expected: 7 tests PASS

- [ ] **Step 8: 创建 types/mod.rs**

File: `common/src/types/mod.rs`

```rust
mod memory;

pub use memory::{Memory, MemoryType};
```

- [ ] **Step 9: 更新 common/src/lib.rs**

File: `common/src/lib.rs`

```rust
pub mod types;

pub use types::{Memory, MemoryType};
```

- [ ] **Step 10: 验证模块导出**

```bash
cargo test -p memrec-common
```

Expected: 所有测试 PASS

- [ ] **Step 11: 提交**

```bash
git add common/src/
git commit -m "feat: define Memory and MemoryType types"
```

---

## Task 4: 定义 Project 和 Config 类型

**Files:**
- Create: `common/src/types/project.rs`
- Create: `common/src/types/config.rs`

- [ ] **Step 1: 定义 Project 类型**

File: `common/src/types/project.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::MemoryConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub config: ProjectConfig,
}

impl Project {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description: None,
            created_at: Utc::now(),
            config: ProjectConfig::default(),
        }
    }
    
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub memory_config: MemoryConfig,
    pub active: bool,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            memory_config: MemoryConfig::default(),
            active: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_project_creation() {
        let project = Project::new("my-project".to_string());
        
        assert!(!project.id.to_string().is_empty());
        assert_eq!(project.name, "my-project");
        assert!(project.description.is_none());
        assert!(!project.config.active);
    }
    
    #[test]
    fn test_project_with_description() {
        let project = Project::new("test".to_string())
            .with_description("Test project".to_string());
        
        assert_eq!(project.description, Some("Test project".to_string()));
    }
    
    #[test]
    fn test_project_serde() {
        let project = Project::new("test".to_string());
        let json = serde_json::to_string(&project).unwrap();
        let parsed: Project = serde_json::from_str(&json).unwrap();
        
        assert_eq!(project.id, parsed.id);
        assert_eq!(project.name, parsed.name);
    }
}
```

- [ ] **Step 2: 定义 MemoryConfig 类型**

File: `common/src/types/config.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub soft_delete_recovery_days: u32,
    pub hard_delete_importance: f32,
    pub hard_delete_inactive_days: u32,
    pub compression_importance: f32,
    pub max_storage_gb: usize,
    pub high_watermark: f32,
    pub low_watermark: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            soft_delete_recovery_days: 30,
            hard_delete_importance: 0.1,
            hard_delete_inactive_days: 90,
            compression_importance: 0.3,
            max_storage_gb: 10,
            high_watermark: 0.9,
            low_watermark: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportanceConfig {
    pub lambda: f32,
    pub frequency_normalize: f32,
    pub weight_recency: f32,
    pub weight_frequency: f32,
    pub weight_semantic: f32,
    pub weight_explicit: f32,
}

impl Default for ImportanceConfig {
    fn default() -> Self {
        Self {
            lambda: 0.05,
            frequency_normalize: 10.0,
            weight_recency: 0.3,
            weight_frequency: 0.2,
            weight_semantic: 0.2,
            weight_explicit: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub socket_path: String,
    pub data_dir: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            socket_path: "~/.memrec/memrecd.sock".to_string(),
            data_dir: "~/.memrec/data".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_memory_config_defaults() {
        let config = MemoryConfig::default();
        
        assert_eq!(config.soft_delete_recovery_days, 30);
        assert_eq!(config.hard_delete_importance, 0.1);
        assert_eq!(config.max_storage_gb, 10);
        assert_eq!(config.high_watermark, 0.9);
    }
    
    #[test]
    fn test_importance_config_defaults() {
        let config = ImportanceConfig::default();
        
        assert_eq!(config.lambda, 0.05);
        assert_eq!(config.weight_recency, 0.3);
    }
    
    #[test]
    fn test_config_serde() {
        let config = MemoryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: MemoryConfig = serde_json::from_str(&json).unwrap();
        
        assert_eq!(config.soft_delete_recovery_days, parsed.soft_delete_recovery_days);
    }
}
```

- [ ] **Step 3: 运行测试**

```bash
cargo test -p memrec-common --lib types
```

Expected: 所有 tests PASS

- [ ] **Step 4: 更新 types/mod.rs 导出**

File: `common/src/types/mod.rs`

```rust
mod memory;
mod project;
mod config;

pub use memory::{Memory, MemoryType};
pub use project::{Project, ProjectConfig};
pub use config::{MemoryConfig, ImportanceConfig, ServerConfig};
```

- [ ] **Step 5: 更新 lib.rs 导出**

File: `common/src/lib.rs`

```rust
pub mod types;

pub use types::{Memory, MemoryType, Project, ProjectConfig};
pub use types::{MemoryConfig, ImportanceConfig, ServerConfig};
```

- [ ] **Step 6: 验证完整导出**

```bash
cargo test -p memrec-common
```

Expected: 所有测试 PASS

- [ ] **Step 7: 提交**

```bash
git add common/src/types/
git commit -m "feat: define Project and Config types"
```

---

## Task 5: 定义 JSON-RPC 协议类型

**Files:**
- Create: `common/src/protocol/mod.rs`
- Create: `common/src/protocol/request.rs`
- Create: `common/src/protocol/response.rs`
- Create: `common/src/protocol/error.rs`

- [ ] **Step 1: 创建 protocol 目录**

```bash
mkdir -p common/src/protocol
```

- [ ] **Step 2: 定义 JSON-RPC 错误类型**

File: `common/src/protocol/error.rs`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemRecError {
    #[error("Memory not found: {0}")]
    MemoryNotFound(uuid::Uuid),
    
    #[error("Project not found: {0}")]
    ProjectNotFound(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Embedding error: {0}")]
    EmbeddingError(String),
    
    #[error("Memory already deleted: {0}")]
    AlreadyDeleted(uuid::Uuid),
    
    #[error("Recovery period expired")]
    RecoveryExpired,
}

impl From<MemRecError> for JsonRpcError {
    fn from(err: MemRecError) -> Self {
        JsonRpcError {
            code: match err {
                MemRecError::MemoryNotFound(_) => -32001,
                MemRecError::ProjectNotFound(_) => -32002,
                MemRecError::StorageError(_) => -32003,
                MemRecError::InvalidRequest(_) => -32600,
                MemRecError::ConnectionError(_) => -32004,
                MemRecError::EmbeddingError(_) => -32005,
                MemRecError::AlreadyDeleted(_) => -32006,
                MemRecError::RecoveryExpired => -32007,
            },
            message: err.to_string(),
            data: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_to_jsonrpc() {
        let err = MemRecError::MemoryNotFound(uuid::Uuid::nil());
        let rpc_err: JsonRpcError = err.into();
        
        assert_eq!(rpc_err.code, -32001);
        assert!(rpc_err.message.contains("not found"));
    }
    
    #[test]
    fn test_jsonrpc_error_serde() {
        let err = JsonRpcError {
            code: -32600,
            message: "Invalid Request".to_string(),
            data: Some("details".to_string()),
        };
        
        let json = serde_json::to_string(&err).unwrap();
        let parsed: JsonRpcError = serde_json::from_str(&json).unwrap();
        
        assert_eq!(err.code, parsed.code);
        assert_eq!(err.data, parsed.data);
    }
}
```

- [ ] **Step 3: 运行错误类型测试**

```bash
cargo test -p memrec-common --lib protocol::error
```

Expected: 2 tests PASS

- [ ] **Step 4: 定义请求类型**

File: `common/src/protocol/request.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::MemoryType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestAction {
    Add,
    Get,
    Update,
    Delete,
    Search,
    List,
    Tag,
    Untag,
    
    ProjectCreate,
    ProjectList,
    ProjectSwitch,
    ProjectDelete,
    
    ConfigGet,
    ConfigSet,
    
    Stats,
    Compress,
    Forget,
    Export,
    Import,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: RequestAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<RequestParams>,
    pub id: u64,
}

impl JsonRpcRequest {
    pub fn new(method: RequestAction, params: Option<RequestParams>, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
            id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestParams {
    Add(AddParams),
    Get(GetParams),
    Update(UpdateParams),
    Delete(DeleteParams),
    Search(SearchParams),
    List(ListParams),
    Tag(TagParams),
    
    ProjectCreate(ProjectCreateParams),
    ProjectSwitch(ProjectSwitchParams),
    ProjectDelete(ProjectDeleteParams),
    
    ConfigSet(ConfigSetParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddParams {
    pub content: String,
    #[serde(default)]
    pub memory_type: MemoryType,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParams {
    pub id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateParams {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteParams {
    pub id: Uuid,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub mode: SearchMode,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<TimeRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default)]
    pub min_importance: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Exact,
    Semantic,
    Hybrid,
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

fn default_top_k() -> usize { 10 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParams {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 20 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagParams {
    pub id: Uuid,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCreateParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSwitchParams {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDeleteParams {
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSetParams {
    pub key: String,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_request_creation() {
        let req = JsonRpcRequest::new(
            RequestAction::Add,
            Some(RequestParams::Add(AddParams {
                content: "test".to_string(),
                memory_type: MemoryType::Knowledge,
                tags: vec!["tag1".to_string()],
                project_id: None,
            })),
            1,
        );
        
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, 1);
    }
    
    #[test]
    fn test_request_serde() {
        let req = JsonRpcRequest::new(
            RequestAction::Get,
            Some(RequestParams::Get(GetParams {
                id: Uuid::nil(),
            })),
            1,
        );
        
        let json = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        
        assert_eq!(req.jsonrpc, parsed.jsonrpc);
        assert_eq!(req.method, parsed.method);
    }
    
    #[test]
    fn test_search_params_defaults() {
        let params = SearchParams {
            text: None,
            mode: SearchMode::default(),
            tags: Vec::new(),
            time_range: None,
            project_id: None,
            top_k: default_top_k(),
            min_importance: 0.0,
        };
        
        assert_eq!(params.mode, SearchMode::Hybrid);
        assert_eq!(params.top_k, 10);
    }
}
```

- [ ] **Step 5: 运行请求类型测试**

```bash
cargo test -p memrec-common --lib protocol::request
```

Expected: 3 tests PASS

- [ ] **Step 6: 定义响应类型**

File: `common/src/protocol/response.rs`

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{Memory, Project};
use super::error::JsonRpcError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ResponseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: u64,
}

impl JsonRpcResponse {
    pub fn success(result: ResponseResult, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }
    
    pub fn error(err: JsonRpcError, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(err),
            id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseResult {
    Memory(MemoryResult),
    MemoryList(MemoryListResult),
    SearchResult(SearchResult),
    Project(ProjectResult),
    ProjectList(ProjectListResult),
    Config(ConfigResult),
    Stats(StatsResult),
    Success(SuccessResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub memory: Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListResult {
    pub memories: Vec<Memory>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memories: Vec<Memory>,
    pub total: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResult {
    pub project: Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectListResult {
    pub projects: Vec<Project>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResult {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub total_memories: usize,
    pub active_memories: usize,
    pub deleted_memories: usize,
    pub storage_usage: f32,
    pub avg_importance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResult {
    pub message: String,
}

impl From<bool> for SuccessResult {
    fn from(success: bool) -> Self {
        Self {
            message: if success { "Success".to_string() } else { "Failed".to_string() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryType;
    
    #[test]
    fn test_success_response() {
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        let resp = JsonRpcResponse::success(
            ResponseResult::Memory(MemoryResult { memory }),
            1,
        );
        
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }
    
    #[test]
    fn test_error_response() {
        let err = JsonRpcError {
            code: -32001,
            message: "Not found".to_string(),
            data: None,
        };
        let resp = JsonRpcResponse::error(err, 1);
        
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }
    
    #[test]
    fn test_response_serde() {
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        let resp = JsonRpcResponse::success(
            ResponseResult::Memory(MemoryResult { memory }),
            1,
        );
        
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        
        assert_eq!(resp.id, parsed.id);
    }
}
```

- [ ] **Step 7: 运行响应类型测试**

```bash
cargo test -p memrec-common --lib protocol::response
```

Expected: 3 tests PASS

- [ ] **Step 8: 创建 protocol/mod.rs**

File: `common/src/protocol/mod.rs`

```rust
mod error;
mod request;
mod response;

pub use error::{MemRecError, JsonRpcError};
pub use request::{
    JsonRpcRequest, RequestAction, RequestParams,
    AddParams, GetParams, UpdateParams, DeleteParams,
    SearchParams, SearchMode, TimeRange,
    ListParams, TagParams,
    ProjectCreateParams, ProjectSwitchParams, ProjectDeleteParams,
    ConfigSetParams,
};
pub use response::{
    JsonRpcResponse, ResponseResult,
    MemoryResult, MemoryListResult, SearchResult,
    ProjectResult, ProjectListResult,
    ConfigResult, StatsResult, SuccessResult,
};
```

- [ ] **Step 9: 更新 lib.rs 导出 protocol**

File: `common/src/lib.rs`

```rust
pub mod types;
pub mod protocol;

pub use types::{Memory, MemoryType, Project, ProjectConfig};
pub use types::{MemoryConfig, ImportanceConfig, ServerConfig};

pub use protocol::{MemRecError, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
```

- [ ] **Step 10: 运行所有测试**

```bash
cargo test -p memrec-common
```

Expected: 所有测试 PASS

- [ ] **Step 11: 提交**

```bash
git add common/src/protocol common/src/lib.rs
git commit -m "feat: define JSON-RPC protocol types"
```

---

## Task 6: 创建 memrecd 和 memrec 占位 crate

**Files:**
- Modify: `memrecd/Cargo.toml`
- Modify: `memrecd/src/main.rs`
- Modify: `memrec/Cargo.toml`
- Modify: `memrec/src/main.rs`

- [ ] **Step 1: 初始化 memrecd crate**

```bash
cargo new memrecd --name memrecd
```

Expected: 创建 memrecd 目录

- [ ] **Step 2: 配置 memrecd/Cargo.toml**

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
```

- [ ] **Step 3: 创建 memrecd 占位 main.rs**

File: `memrecd/src/main.rs`

```rust
use anyhow::Result;

fn main() -> Result<()> {
    println!("memrecd - Memory persistence daemon");
    println!("Phase 1 placeholder - full implementation in Phase 3");
    Ok(())
}
```

- [ ] **Step 4: 验证 memrecd 编译**

```bash
cargo build -p memrecd
```

Expected: PASS

- [ ] **Step 5: 初始化 memrec crate**

```bash
cargo new memrec --name memrec
```

Expected: 创建 memrec 目录

- [ ] **Step 6: 配置 memrec/Cargo.toml**

```toml
[package]
name = "memrec"
version.workspace = true
edition.workspace = true

[[bin]]
name = "memrec"
path = "src/main.rs"

[dependencies]
memrec-common = { path = "../common" }
anyhow.workspace = true
```

- [ ] **Step 7: 创建 memrec 占位 main.rs**

File: `memrec/src/main.rs`

```rust
use anyhow::Result;

fn main() -> Result<()> {
    println!("memrec - Memory persistence CLI");
    println!("Phase 1 placeholder - full implementation in Phase 4");
    Ok(())
}
```

- [ ] **Step 8: 验证整个 workspace 编译**

```bash
cargo build
```

Expected: 所有 targets PASS

- [ ] **Step 9: 提交**

```bash
git add memrecd/ memrec/
git commit -m "feat: add memrecd and memrec placeholder crates"
```

---

## Task 7: 最终验证和文档

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 创建 README.md**

```markdown
# MemRec - AI CLI 记忆持久化系统

本地化的记忆持久化系统，为 AI CLI 工具（如 opencode）提供跨会话记忆恢复、知识库积累、对话历史存档能力。

## 项目结构

```
memrec/
├── common/       # 共享类型和协议定义
├── memrecd/      # 守护进程服务
├── memrec/       # CLI 工具
└── docs/         # 设计文档
```

## 构建状态

Phase 1 (基础设施): ✅ 完成
- Workspace 结构
- Memory/Project/Config 类型
- JSON-RPC 协议

## 构建

```bash
cargo build
cargo test
```

## 文档

- [设计文档](docs/superpowers/specs/2026-04-23-memrec-design.md)
- [算法文档](docs/superpowers/specs/2026-04-23-memrec-algorithms.md)
```

- [ ] **Step 2: 运行完整测试套件**

```bash
cargo test --workspace
```

Expected: 所有测试 PASS

- [ ] **Step 3: 检查代码质量**

```bash
cargo clippy --workspace
```

Expected: 无 warnings

- [ ] **Step 4: 提交 README**

```bash
git add README.md
git commit -m "docs: add project README"
```

- [ ] **Step 5: Phase 1 完成**

```bash
git log --oneline -5
```

Expected: 看到 Phase 1 的所有 commit

---

## Phase 1 完成检查清单

- [x] Workspace 结构建立
- [x] common crate 配置
- [x] Memory 类型定义（含测试）
- [x] MemoryType 枚举（含测试）
- [x] Project 类型（含测试）
- [x] MemoryConfig/ImportanceConfig 类型（含测试）
- [x] JSON-RPC 错误类型（含测试）
- [x] JSON-RPC 请求类型（含测试）
- [x] JSON-RPC 响应类型（含测试）
- [x] memrecd 占位 crate
- [x] memrec 占位 crate
- [x] 所有测试通过
- [x] README 文档

**下一阶段:** Phase 2 - 存储层实现（RocksDB + usearch）