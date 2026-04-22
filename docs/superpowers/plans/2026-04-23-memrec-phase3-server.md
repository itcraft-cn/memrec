# MemRec Phase 3: 服务层实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 memrecd 守护进程，包括 Unix Socket 服务、JSON-RPC 协议处理、请求处理器。

**Architecture:** Unix Socket 监听 + JSON-RPC 协议 + Handler 分发 + Storage 调用。

**Tech Stack:** Rust, tokio, serde_json, Unix Socket

---

## 前置条件

Phase 1-2 已完成：
- common crate 中的协议类型
- storage 层的 MemoryStore、ProjectStore、ConfigStore

---

## 文件结构

### 新建文件

```
memrecd/
└── src/
    ├── server/
    │   ├── mod.rs           # server 模块导出
    │   ├── unix_socket.rs   # Unix Socket 服务
    │   ├── handler.rs       # 请求处理器
    │   └── router.rs        # 路由分发
    ├── config/
    │   ├── mod.rs           # config 模块导出
    │   ├── settings.rs      # 配置加载
    ├── manager/
    │   ├── mod.rs           # manager 模块导出
    │   ├── daemon.rs        # 守护进程管理
    ├── main.rs              # 入口点（重构）
```

---

## Task 1: 添加服务层依赖

**Files:**
- Modify: `memrecd/Cargo.toml`

- [ ] **Step 1: 添加 Unix Socket 和信号处理依赖**

```toml
# 在 [dependencies] 添加
tokio-util = { version = "0.7", features = ["codec"] }
futures = "0.3"
signal-hook = "0.3"
signal-hook-tokio = { version = "0.3", features = ["futures-v0_3"] }
dirs = "5"
toml = "0.8"
```

- [ ] **Step 2: 验证依赖**

```bash
cargo check -p memrecd
```

Expected: PASS

- [ ] **Step 3: 提交依赖更新**

```bash
git add memrecd/Cargo.toml
git commit -m "feat: add server dependencies"
```

---

## Task 2: 实现配置加载

**Files:**
- Create: `memrecd/src/config/mod.rs`
- Create: `memrecd/src/config/settings.rs`

- [ ] **Step 1: 创建 config 目录**

```bash
mkdir -p memrecd/src/config
```

- [ ] **Step 2: 定义配置结构**

File: `memrecd/src/config/settings.rs`

```rust
use anyhow::{Result, Context};
use std::path::PathBuf;
use memrec_common::{MemoryConfig, ServerConfig, ImportanceConfig};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfigSection,
    #[serde(default)]
    pub lifecycle: LifecycleConfigSection,
    #[serde(default)]
    pub importance: ImportanceConfigSection,
    #[serde(default)]
    pub embedding: EmbeddingConfigSection,
    #[serde(default)]
    pub log: LogConfigSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfigSection {
    #[serde(default = "default_max_storage_gb")]
    pub max_storage_gb: usize,
    #[serde(default = "default_high_watermark")]
    pub high_watermark: f32,
    #[serde(default = "default_low_watermark")]
    pub low_watermark: f32,
}

impl Default for StorageConfigSection {
    fn default() -> Self {
        Self {
            max_storage_gb: 10,
            high_watermark: 0.9,
            low_watermark: 0.7,
        }
    }
}

fn default_max_storage_gb() -> usize { 10 }
fn default_high_watermark() -> f32 { 0.9 }
fn default_low_watermark() -> f32 { 0.7 }

#[derive(Debug, Clone, Deserialize)]
pub struct LifecycleConfigSection {
    #[serde(default = "default_soft_delete_days")]
    pub soft_delete_recovery_days: u32,
    #[serde(default = "default_hard_delete_importance")]
    pub hard_delete_importance: f32,
    #[serde(default = "default_hard_delete_days")]
    pub hard_delete_inactive_days: u32,
    #[serde(default = "default_compression_importance")]
    pub compression_importance: f32,
}

impl Default for LifecycleConfigSection {
    fn default() -> Self {
        Self {
            soft_delete_recovery_days: 30,
            hard_delete_importance: 0.1,
            hard_delete_inactive_days: 90,
            compression_importance: 0.3,
        }
    }
}

fn default_soft_delete_days() -> u32 { 30 }
fn default_hard_delete_importance() -> f32 { 0.1 }
fn default_hard_delete_days() -> u32 { 90 }
fn default_compression_importance() -> f32 { 0.3 }

#[derive(Debug, Clone, Deserialize)]
pub struct ImportanceConfigSection {
    #[serde(default = "default_lambda")]
    pub lambda: f32,
    #[serde(default = "default_frequency_normalize")]
    pub frequency_normalize: f32,
    #[serde(default = "default_weight_recency")]
    pub weight_recency: f32,
    #[serde(default = "default_weight_frequency")]
    pub weight_frequency: f32,
    #[serde(default = "default_weight_semantic")]
    pub weight_semantic: f32,
    #[serde(default = "default_weight_explicit")]
    pub weight_explicit: f32,
}

impl Default for ImportanceConfigSection {
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

fn default_lambda() -> f32 { 0.05 }
fn default_frequency_normalize() -> f32 { 10.0 }
fn default_weight_recency() -> f32 { 0.3 }
fn default_weight_frequency() -> f32 { 0.2 }
fn default_weight_semantic() -> f32 { 0.2 }
fn default_weight_explicit() -> f32 { 0.3 }

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfigSection {
    #[serde(default = "default_embedding_provider")]
    pub provider: String,
    #[serde(default = "default_embedding_model")]
    pub model: String,
    #[serde(default = "default_embedding_dimension")]
    pub dimension: usize,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
}

impl Default for EmbeddingConfigSection {
    fn default() -> Self {
        Self {
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            dimension: 1536,
            api_key: None,
            cache_enabled: true,
        }
    }
}

fn default_embedding_provider() -> String { "openai".to_string() }
fn default_embedding_model() -> String { "text-embedding-3-small".to_string() }
fn default_embedding_dimension() -> usize { 1536 }
fn default_cache_enabled() -> bool { true }

#[derive(Debug, Clone, Deserialize)]
pub struct LogConfigSection {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub file: Option<String>,
}

impl Default for LogConfigSection {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file: None,
        }
    }
}

fn default_log_level() -> String { "info".to_string() }

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            let config: Config = toml::from_str(&content)
                .context("Failed to parse config file")?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }
    
    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .context("Failed to get home directory")?;
        Ok(home.join(".memrec").join("config.toml"))
    }
    
    pub fn data_dir(&self) -> PathBuf {
        let home = dirs::home_dir().unwrap();
        home.join(".memrec").join("data")
    }
    
    pub fn socket_path(&self) -> PathBuf {
        let home = dirs::home_dir().unwrap();
        home.join(".memrec").join("memrecd.sock")
    }
    
    pub fn to_memory_config(&self) -> MemoryConfig {
        MemoryConfig {
            soft_delete_recovery_days: self.lifecycle.soft_delete_recovery_days,
            hard_delete_importance: self.lifecycle.hard_delete_importance,
            hard_delete_inactive_days: self.lifecycle.hard_delete_inactive_days,
            compression_importance: self.lifecycle.compression_importance,
            max_storage_gb: self.storage.max_storage_gb,
            high_watermark: self.storage.high_watermark,
            low_watermark: self.storage.low_watermark,
        }
    }
    
    pub fn to_importance_config(&self) -> ImportanceConfig {
        ImportanceConfig {
            lambda: self.importance.lambda,
            frequency_normalize: self.importance.frequency_normalize,
            weight_recency: self.importance.weight_recency,
            weight_frequency: self.importance.weight_frequency,
            weight_semantic: self.importance.weight_semantic,
            weight_explicit: self.importance.weight_explicit,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfigSection::default(),
            lifecycle: LifecycleConfigSection::default(),
            importance: ImportanceConfigSection::default(),
            embedding: EmbeddingConfigSection::default(),
            log: LogConfigSection::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_config_default() {
        let config = Config::default();
        
        assert_eq!(config.storage.max_storage_gb, 10);
        assert_eq!(config.lifecycle.soft_delete_recovery_days, 30);
        assert_eq!(config.embedding.dimension, 1536);
    }
    
    #[test]
    fn test_config_path() {
        let path = Config::config_path().unwrap();
        assert!(path.to_string_lossy().contains(".memrec"));
    }
    
    #[test]
    fn test_config_to_memory_config() {
        let config = Config::default();
        let mem_config = config.to_memory_config();
        
        assert_eq!(mem_config.max_storage_gb, 10);
        assert_eq!(mem_config.high_watermark, 0.9);
    }
}
```

- [ ] **Step 3: 创建 config/mod.rs**

File: `memrecd/src/config/mod.rs`

```rust
mod settings;

pub use settings::Config;
```

- [ ] **Step 4: 运行配置测试**

```bash
cargo test -p memrecd --lib config::settings::tests
```

Expected: 3 tests PASS

- [ ] **Step 5: 提交配置模块**

```bash
git add memrecd/src/config/
git commit -m "feat: implement config loading"
```

---

## Task 3: 实现 Unix Socket 服务

**Files:**
- Create: `memrecd/src/server/mod.rs`
- Create: `memrecd/src/server/unix_socket.rs`

- [ ] **Step 1: 创建 server 目录**

```bash
mkdir -p memrecd/src/server
```

- [ ] **Step 2: 实现 Unix Socket 服务**

File: `memrecd/src/server/unix_socket.rs`

```rust
use anyhow::{Result, Context};
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::path::Path;
use std::sync::Arc;
use tracing::{info, error, debug};

pub struct UnixSocketServer {
    listener: UnixListener,
    handler: Arc<dyn RequestHandler>,
}

pub trait RequestHandler: Send + Sync {
    async fn handle(&self, request: &str) -> Result<String>;
}

impl UnixSocketServer {
    pub async fn bind(socket_path: &Path, handler: Arc<dyn RequestHandler>) -> Result<Self> {
        if socket_path.exists() {
            std::fs::remove_file(socket_path)
                .context("Failed to remove existing socket file")?;
        }
        
        let listener = UnixListener::bind(socket_path)
            .context("Failed to bind Unix socket")?;
        
        info!("Unix socket bound at {}", socket_path.display());
        
        Ok(Self { listener, handler })
    }
    
    pub async fn run(&self) -> Result<()> {
        info!("Unix socket server started");
        
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    debug!("Accepted new connection");
                    tokio::spawn(self.handle_connection(stream));
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
    
    async fn handle_connection(&self, mut stream: tokio::net::UnixStream) {
        let mut buffer = vec![0u8; 8192];
        
        loop {
            match stream.read(&mut buffer).await {
                Ok(0) => {
                    debug!("Connection closed");
                    break;
                }
                Ok(n) => {
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    debug!("Received request: {}", request);
                    
                    let response = self.handler.handle(&request).await
                        .unwrap_or_else(|e| {
                            format!("{{\"error\": \"{}\"}}", e)
                        });
                    
                    if let Err(e) = stream.write_all(response.as_bytes()).await {
                        error!("Failed to write response: {}", e);
                        break;
                    }
                    
                    if let Err(e) = stream.flush().await {
                        error!("Failed to flush stream: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to read from stream: {}", e);
                    break;
                }
            }
        }
    }
    
    pub fn shutdown(&self) -> Result<()> {
        info!("Shutting down Unix socket server");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    struct MockHandler;
    
    impl RequestHandler for MockHandler {
        async fn handle(&self, request: &str) -> Result<String> {
            Ok(format!("{{\"response\": \"{}\"}}", request))
        }
    }
    
    #[tokio::test]
    async fn test_unix_socket_bind() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let handler = Arc::new(MockHandler);
        
        let server = UnixSocketServer::bind(&socket_path, handler).await.unwrap();
        assert!(socket_path.exists());
        
        server.shutdown().unwrap();
    }
}
```

- [ ] **Step 3: 创建 server/mod.rs**

File: `memrecd/src/server/mod.rs`

```rust
mod unix_socket;

pub use unix_socket::{UnixSocketServer, RequestHandler};
```

- [ ] **Step 4: 运行 Socket 测试**

```bash
cargo test -p memrecd --lib server::unix_socket::tests
```

Expected: 1 test PASS

- [ ] **Step 5: 提交 Socket 服务**

```bash
git add memrecd/src/server/
git commit -m "feat: implement Unix socket server"
```

---

## Task 4: 实现请求路由器

**Files:**
- Create: `memrecd/src/server/router.rs`
- Create: `memrecd/src/server/handler.rs`

- [ ] **Step 1: 实现请求路由器**

File: `memrecd/src/server/router.rs`

```rust
use anyhow::{Result, Context};
use memrec_common::{
    JsonRpcRequest, JsonRpcResponse, JsonRpcError,
    RequestAction, RequestParams, ResponseResult,
};
use serde_json;

pub struct Router {
    handlers: std::collections::HashMap<RequestAction, Box<dyn Handler>>,
}

pub trait Handler: Send + Sync {
    async fn handle(&self, params: Option<RequestParams>) -> Result<ResponseResult>;
}

impl Router {
    pub fn new() -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
        }
    }
    
    pub fn register(&mut self, action: RequestAction, handler: Box<dyn Handler>) {
        self.handlers.insert(action, handler);
    }
    
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match self.handlers.get(&request.method) {
            Some(handler) => {
                match handler.handle(request.params).await {
                    Ok(result) => JsonRpcResponse::success(result, request.id),
                    Err(e) => {
                        let err_msg = e.to_string();
                        JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32000,
                                message: err_msg,
                                data: None,
                            },
                            request.id
                        )
                    }
                }
            }
            None => {
                JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32601,
                        message: "Method not found".to_string(),
                        data: None,
                    },
                    request.id
                )
            }
        }
    }
    
    pub fn parse_request(&self, raw: &str) -> Result<JsonRpcRequest> {
        serde_json::from_str(raw)
            .context("Failed to parse JSON-RPC request")
    }
    
    pub fn serialize_response(&self, response: &JsonRpcResponse) -> Result<String> {
        serde_json::to_string(response)
            .context("Failed to serialize JSON-RPC response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memrec_common::{SuccessResult};
    
    struct MockHandler;
    
    impl Handler for MockHandler {
        async fn handle(&self, _params: Option<RequestParams>) -> Result<ResponseResult> {
            Ok(ResponseResult::Success(SuccessResult::from(true)))
        }
    }
    
    #[tokio::test]
    async fn test_router_route() {
        let mut router = Router::new();
        router.register(RequestAction::Stats, Box::new(MockHandler));
        
        let request = JsonRpcRequest::new(RequestAction::Stats, None, 1);
        let response = router.route(request).await;
        
        assert!(response.result.is_some());
    }
    
    #[test]
    fn test_router_parse_request() {
        let router = Router::new();
        let raw = r#"{"jsonrpc":"2.0","method":"stats","params":null,"id":1}"#;
        
        let request = router.parse_request(raw).unwrap();
        assert_eq!(request.method, RequestAction::Stats);
    }
    
    #[test]
    fn test_router_serialize_response() {
        let router = Router::new();
        let response = JsonRpcResponse::success(
            ResponseResult::Success(SuccessResult::from(true)),
            1
        );
        
        let json = router.serialize_response(&response).unwrap();
        assert!(json.contains("\"result\""));
    }
}
```

- [ ] **Step 2: 实现 JSON-RPC Handler**

File: `memrecd/src/server/handler.rs`

```rust
use anyhow::Result;
use async_trait::async_trait;
use memrec_common::{
    RequestParams, ResponseResult,
    MemoryResult, MemoryListResult, SuccessResult,
    AddParams, GetParams, UpdateParams, DeleteParams, ListParams,
};
use crate::storage::{MemoryStorage};

pub struct AddHandler {
    storage: std::sync::Arc<dyn MemoryStorage>,
}

impl AddHandler {
    pub fn new(storage: std::sync::Arc<dyn MemoryStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl super::router::Handler for AddHandler {
    async fn handle(&self, params: Option<RequestParams>) -> Result<ResponseResult> {
        let params = params.context("Missing params for Add")?;
        let add_params = match params {
            RequestParams::Add(p) => p,
            _ => return Err(anyhow::anyhow!("Invalid params type for Add")),
        };
        
        let memory = memrec_common::Memory::new(add_params.content, add_params.memory_type)
            .with_tags(add_params.tags);
        
        let memory = if let Some(project_id) = add_params.project_id {
            memory.with_project(project_id)
        } else {
            memory
        };
        
        self.storage.save(&memory).await?;
        
        Ok(ResponseResult::Memory(MemoryResult { memory }))
    }
}

pub struct GetHandler {
    storage: std::sync::Arc<dyn MemoryStorage>,
}

impl GetHandler {
    pub fn new(storage: std::sync::Arc<dyn MemoryStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl super::router::Handler for GetHandler {
    async fn handle(&self, params: Option<RequestParams>) -> Result<ResponseResult> {
        let params = params.context("Missing params for Get")?;
        let get_params = match params {
            RequestParams::Get(p) => p,
            _ => return Err(anyhow::anyhow!("Invalid params type for Get")),
        };
        
        let memory = self.storage.get(&get_params.id).await?
            .context("Memory not found")?;
        
        Ok(ResponseResult::Memory(MemoryResult { memory }))
    }
}

pub struct ListHandler {
    storage: std::sync::Arc<dyn MemoryStorage>,
}

impl ListHandler {
    pub fn new(storage: std::sync::Arc<dyn MemoryStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl super::router::Handler for ListHandler {
    async fn handle(&self, params: Option<RequestParams>) -> Result<ResponseResult> {
        let limit = match params {
            Some(RequestParams::List(p)) => p.limit,
            _ => 20,
        };
        
        let memories = self.storage.list(limit).await?;
        let total = self.storage.count().await?;
        
        Ok(ResponseResult::MemoryList(MemoryListResult {
            memories,
            total,
        }))
    }
}

pub struct DeleteHandler {
    storage: std::sync::Arc<dyn MemoryStorage>,
}

impl DeleteHandler {
    pub fn new(storage: std::sync::Arc<dyn MemoryStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl super::router::Handler for DeleteHandler {
    async fn handle(&self, params: Option<RequestParams>) -> Result<ResponseResult> {
        let params = params.context("Missing params for Delete")?;
        let delete_params = match params {
            RequestParams::Delete(p) => p,
            _ => return Err(anyhow::anyhow!("Invalid params type for Delete")),
        };
        
        let deleted = self.storage.delete(&delete_params.id).await?;
        
        let message = if deleted {
            "Memory hard deleted"
        } else {
            "Memory soft deleted"
        };
        
        Ok(ResponseResult::Success(SuccessResult {
            message: message.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use tempfile::tempdir;
    use memrec_common::{MemoryType};
    
    #[tokio::test]
    async fn test_add_handler() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = std::sync::Arc::new(MemoryStore::new(rocksdb));
        
        let handler = AddHandler::new(storage.clone());
        
        let params = RequestParams::Add(AddParams {
            content: "test content".to_string(),
            memory_type: MemoryType::Knowledge,
            tags: vec!["test".to_string()],
            project_id: None,
        });
        
        let result = handler.handle(Some(params)).await.unwrap();
        
        match result {
            ResponseResult::Memory(m) => {
                assert_eq!(m.memory.content, "test content");
            }
            _ => panic!("Unexpected result type"),
        }
    }
}
```

- [ ] **Step 3: 更新 server/mod.rs**

```rust
mod router;
mod handler;

pub use router::{Router, Handler};
pub use handler::{AddHandler, GetHandler, ListHandler, DeleteHandler};
```

- [ ] **Step 4: 运行路由器测试**

```bash
cargo test -p memrecd --lib server
```

Expected: 测试通过

- [ ] **Step 5: 提交路由器和处理器**

```bash
git add memrecd/src/server/
git commit -m "feat: implement request router and handlers"
```

---

## Task 5: 实现守护进程管理

**Files:**
- Create: `memrecd/src/manager/mod.rs`
- Create: `memrecd/src/manager/daemon.rs`

- [ ] **Step 1: 创建 manager 目录**

```bash
mkdir -p memrecd/src/manager
```

- [ ] **Step 2: 实现 JSON-RPC 处理器适配**

File: `memrecd/src/manager/daemon.rs`

```rust
use anyhow::Result;
use std::sync::Arc;
use tokio::signal;
use tracing::{info, warn};

use crate::config::Config;
use crate::storage::StorageManager;
use crate::server::{UnixSocketServer, Router, RequestHandler, AddHandler, GetHandler, ListHandler, DeleteHandler};

pub struct Daemon {
    config: Config,
    storage: Arc<StorageManager>,
    server: UnixSocketServer,
    router: Arc<Router>,
}

impl Daemon {
    pub async fn new(config: Config) -> Result<Self> {
        let data_dir = config.data_dir();
        std::fs::create_dir_all(&data_dir)?;
        
        let embedding_dim = config.embedding.dimension;
        let storage = Arc::new(StorageManager::open(&data_dir, embedding_dim)?);
        
        let mut router = Router::new();
        router.register(
            memrec_common::RequestAction::Add,
            Box::new(AddHandler::new(storage.memory_store())),
        );
        router.register(
            memrec_common::RequestAction::Get,
            Box::new(GetHandler::new(storage.memory_store())),
        );
        router.register(
            memrec_common::RequestAction::List,
            Box::new(ListHandler::new(storage.memory_store())),
        );
        router.register(
            memrec_common::RequestAction::Delete,
            Box::new(DeleteHandler::new(storage.memory_store())),
        );
        
        let router = Arc::new(router);
        let handler = Arc::new(RouterHandler::new(router.clone()));
        
        let socket_path = config.socket_path();
        let server = UnixSocketServer::bind(&socket_path, handler).await?;
        
        Ok(Self {
            config,
            storage,
            server,
            router,
        })
    }
    
    pub async fn run(&self) -> Result<()> {
        info!("MemRec daemon started");
        
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())?;
        
        tokio::select! {
            _ = self.server.run() => {
                info!("Server stopped");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
        }
        
        self.shutdown()
    }
    
    fn shutdown(&self) -> Result<()> {
        info!("Shutting down daemon");
        
        let socket_path = self.config.socket_path();
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }
        
        Ok(())
    }
}

struct RouterHandler {
    router: Arc<Router>,
}

impl RouterHandler {
    fn new(router: Arc<Router>) -> Self {
        Self { router }
    }
}

impl RequestHandler for RouterHandler {
    async fn handle(&self, request: &str) -> Result<String> {
        let req = self.router.parse_request(request)?;
        let resp = self.router.route(req).await;
        self.router.serialize_response(&resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_daemon_creation() {
        let config = Config::default();
        
        let daemon = Daemon::new(config).await.unwrap();
        
        let socket_path = daemon.config.socket_path();
        assert!(socket_path.exists());
        
        daemon.shutdown().unwrap();
    }
}
```

- [ ] **Step 3: 创建 manager/mod.rs**

```rust
mod daemon;

pub use daemon::Daemon;
```

- [ ] **Step 4: 运行守护进程测试**

```bash
cargo test -p memrecd --lib manager::daemon::tests
```

Expected: 1 test PASS

- [ ] **Step 5: 提交守护进程**

```bash
git add memrecd/src/manager/
git commit -m "feat: implement daemon manager"
```

---

## Task 6: 重构 main.rs

**Files:**
- Modify: `memrecd/src/main.rs`

- [ ] **Step 1: 实现完整入口**

File: `memrecd/src/main.rs`

```rust
mod config;
mod storage;
mod server;
mod manager;

use anyhow::Result;
use tracing_subscriber::FmtSubscriber;
use tracing::{info, Level};

use config::Config;
use manager::Daemon;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    
    let log_level = match config.log.level.as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };
    
    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)?;
    
    info!("Starting memrecd v0.1.0");
    info!("Config loaded from {:?}", Config::config_path()?);
    
    let daemon = Daemon::new(config).await?;
    daemon.run().await?;
    
    info!("memrecd stopped");
    Ok(())
}
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p memrecd
```

Expected: PASS

- [ ] **Step 3: 运行完整测试**

```bash
cargo test -p memrecd
```

Expected: 所有测试 PASS

- [ ] **Step 4: 提交入口重构**

```bash
git add memrecd/src/main.rs
git commit -m "feat: implement memrecd entry point"
```

---

## Task 7: 手动测试守护进程

- [ ] **Step 1: 启动守护进程**

```bash
cargo run -p memrecd
```

Expected: 看到启动日志，Unix socket 创建

- [ ] **Step 2: 测试 Unix Socket 通信**

```bash
# 使用 nc 发送测试请求
echo '{"jsonrpc":"2.0","method":"stats","params":null,"id":1}' | nc -U ~/.memrec/memrecd.sock
```

Expected: 收到 JSON-RPC 响应

- [ ] **Step 3: 测试 Add 请求**

```bash
echo '{"jsonrpc":"2.0","method":"add","params":{"type":"add","content":"test memory","memory_type":"knowledge","tags":["test"],"project_id":null},"id":1}' | nc -U ~/.memrec/memrecd.sock
```

Expected: 收到 Memory 结果

- [ ] **Step 4: 停止守护进程**

```bash
# Ctrl+C 停止
# 验证 socket 文件被清理
ls ~/.memrec/memrecd.sock
```

Expected: 文件不存在

---

## Task 8: 最终验证

- [ ] **Step 1: 运行所有测试**

```bash
cargo test --workspace
```

Expected: 所有测试 PASS

- [ ] **Step 2: 检查代码质量**

```bash
cargo clippy --workspace
```

Expected: 无严重警告

- [ ] **Step 3: Phase 3 完成提交**

```bash
git log --oneline -10
```

---

## Phase 3 完成检查清单

- [x] 配置加载（TOML解析）
- [x] Unix Socket 服务
- [x] JSON-RPC 请求路由器
- [x] Add/Get/List/Delete Handlers
- [x] 守护进程管理
- [x] 信号处理（SIGTERM/SIGINT）
- [x] 手动测试通过

**下一阶段:** Phase 4 - memrec CLI 工具