# Crate 重构设计：拆分 memrecd 为 mr-ability + memrecd

## 背景

当前项目结构：
- `common/` - 共享类型 + JSON-RPC 协议
- `memrecd/` - 守护进程（全部业务逻辑 + 存储层 + daemon 壳）
- `memrec/` - CLI 客户端
- `mr-install/` - 安装器

目标：
1. 让 `../hydrakiller` 项目可以直接依赖全功能库，不需要通过 JSON-RPC
2. 清晰分离：数据类型、协议层、业务逻辑、守护进程壳

## 重构后架构

```
memrec/
├── mr-common/       # 纯数据类型
├── mr-protocol/     # JSON-RPC 协议
├── mr-ability/      # 全功能库
├── memrecd/         # 守护进程壳
├── memrec/          # CLI
└── mr-install/      # 安装器
```

### 依赖关系

```
mr-common ←── mr-protocol
mr-common ←── mr-ability
mr-ability ←── memrecd
mr-protocol ←── memrecd
mr-common ←── memrec
mr-protocol ←── memrec
mr-common ←── mr-install
```

---

## 1. mr-common

**职责**：纯数据类型，无业务逻辑，无协议层

### 目录结构

```
mr-common/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   └── types/
│       ├── mod.rs
│       ├── memory.rs
│       ├── edge.rs
│       ├── facet.rs
│       ├── propagation.rs
│       ├── rule.rs
│       ├── tiered.rs
│       ├── project.rs
│       ├── config.rs
│       └── model.rs
```

### 内容

从 `common/src/types/` 移入：

- `memory.rs` - Memory, MemoryType, MemoryScope, MemorySource, BoostType
- `edge.rs` - MemoryEdge, EdgeType, EdgeHint
- `facet.rs` - MemoryFacet
- `propagation.rs` - PropagationDelta, PropagationConfig, DeltaType
- `rule.rs` - RetrievalRule, RuleCondition, RuleAction, RuleStats
- `tiered.rs` - TieredMemory, TieredSearchResult, TierThresholds, RepresentationTier
- `project.rs` - Project, ProjectConfig
- `config.rs` - MemoryConfig, DreamConfig, ImportanceConfig, SearchConfig, SourceWeights, ServerConfig
- `model.rs` - ModelFile, ModelType, PoolingStrategy

### Cargo.toml

```toml
[package]
name = "mr-common"
version = "0.6.0-dev"
edition = "2021"

[dependencies]
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
anyhow.workspace = true
thiserror.workspace = true
```

### lib.rs

```rust
pub mod types;

pub use types::{
    BoostType, DeltaType, EdgeHint, EdgeType, Memory, MemoryEdge, MemoryFacet, MemoryScope,
    MemorySource, MemoryType, Project, ProjectConfig, PropagationConfig, PropagationDelta,
    RepresentationTier, RetrievalRule, RuleAction, RuleCondition, RuleStats, TierThresholds,
    TieredMemory, TieredSearchResult, TimeRange,
};
pub use types::{DreamConfig, ImportanceConfig, MemoryConfig, SearchConfig, SourceWeights, ServerConfig};
pub use types::{ModelConfig, ModelFile, ModelFileType, ModelType, PoolingStrategy};
```

---

## 2. mr-protocol

**职责**：JSON-RPC 2.0 协议类型

### 目录结构

```
mr-protocol/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── error.rs
│   ├── request.rs
│   └── response.rs
```

### 内容

从 `common/src/protocol/` 移入：

- `error.rs` - JsonRpcError, MemRecError
- `request.rs` - JsonRpcRequest, RequestAction, RequestParams, AddParams, GetParams, ListParams, SearchMemoryParams
- `response.rs` - JsonRpcResponse, ResponseResult, SuccessResult, MemoryResult, MemoryListResult, StatsResult, SearchHit, SemanticSearchResult, ProjectInfoResult

### Cargo.toml

```toml
[package]
name = "mr-protocol"
version = "0.6.0-dev"
edition = "2021"

[dependencies]
mr-common = { path = "../mr-common" }
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
anyhow.workspace = true
thiserror.workspace = true
```

### lib.rs

```rust
pub mod error;
pub mod request;
pub mod response;

pub use error::{JsonRpcError, MemRecError};
pub use request::{
    AddParams, GetParams, JsonRpcRequest, ListParams, RequestAction, RequestParams,
    SearchMemoryParams,
};
pub use response::{
    JsonRpcResponse, MemoryListResult, MemoryResult, ProjectInfoResult, ResponseResult,
    SearchHit, SemanticSearchResult, StatsResult, SuccessResult,
};
pub use request::{default_hybrid_alpha, default_include_global, default_min_score, default_mmr_enabled, default_mmr_lambda};
```

---

## 3. mr-ability

**职责**：全功能库，包含所有业务逻辑、存储层、核心能力

### 目录结构

```
mr-ability/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── project.rs
│   ├── archive/
│   │   ├── mod.rs
│   │   └── sqlite.rs
│   ├── dream/
│   │   ├── mod.rs
│   │   ├── processor.rs
│   │   └── gate.rs
│   ├── edge/
│   │   ├── mod.rs
│   │   └── generator.rs
│   ├── embedding/
│   │   ├── mod.rs
│   │   └── fastembed.rs
│   ├── facet/
│   │   ├── mod.rs
│   │   └── generator.rs
│   ├── importance/
│   │   └── mod.rs
│   ├── lifecycle/
│   │   └── mod.rs
│   ├── propagation/
│   │   ├── mod.rs
│   │   └── engine.rs
│   ├── rule/
│   │   ├── mod.rs
│   │   └── engine.rs
│   ├── search/
│   │   ├── mod.rs
│   │   ├── scorer.rs
│   │   └── mmr.rs
│   ├── tiered/
│   │   ├── mod.rs
│   │   └── engine.rs
│   └── storage/
│       ├── mod.rs
│       ├── traits.rs
│       ├── rocksdb.rs
│       ├── memory_store.rs
│       ├── edge_store.rs
│       ├── facet_store.rs
│       ├── hybrid_store.rs
│       ├── vector_store.rs
│       ├── rocksdb_vector.rs
│       ├── tantivy_store.rs
│       ├── propagation_store.rs
│       ├── rule_store.rs
│       └── tiered_store.rs
└── tests/
    └── hybrid_search_integration.rs
```

### 内容

从 `memrecd/src/` 移入：

**核心能力**（全部）：
- `dream/` - DreamProcessor, DreamGate, DreamLock
- `edge/` - EdgeGenerator trait, ProjectEdgeGenerator, MockEdgeGenerator
- `embedding/` - EmbeddingGenerator trait, FastEmbedGenerator, GeneratorFactory
- `facet/` - FacetGenerator trait, MockFacetGenerator
- `importance/` - ImportanceCalculator
- `lifecycle/` - LifecycleManager
- `propagation/` - PropagationEngine, DeltaGenerator trait, MockDeltaGenerator
- `rule/` - RuleEngine, RuleMatcher
- `search/` - ScorerConfig, MmrConfig, apply_scoring, mmr_rerank
- `tiered/` - TieredEngine, TierAssigner, BudgetController

**存储层**（全部）：
- `storage/` - 所有子模块

**归档**（全部）：
- `archive/`

**项目检测**（全部）：
- `project/`

### Cargo.toml

```toml
[package]
name = "mr-ability"
version = "0.6.0-dev"
edition = "2021"

[lib]
name = "mr_ability"
path = "src/lib.rs"

[dependencies]
mr-common = { path = "../mr-common" }
anyhow.workspace = true
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
tokio = { version = "1", features = ["full"] }
rocksdb = "0.22"
tracing = "0.1"
async-trait = "0.1"
dirs = "5"
fastembed.workspace = true
toml = "0.8"
tantivy = "0.22"
rusqlite = { version = "0.32", features = ["bundled"] }
regex = "1"

[dev-dependencies]
tempfile = "3"
```

### lib.rs

```rust
pub mod archive;
pub mod dream;
pub mod edge;
pub mod embedding;
pub mod facet;
pub mod importance;
pub mod lifecycle;
pub mod project;
pub mod propagation;
pub mod rule;
pub mod search;
pub mod storage;
pub mod tiered;
```

---

## 4. memrecd（重构后）

**职责**：守护进程壳，仅包含 daemon + server + 专属配置

### 目录结构

```
memrecd/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── config.rs
│   ├── daemon/
│   │   └── mod.rs
│   └── server/
│       └── mod.rs
└── tests/
```

### 内容

保留：
- `daemon/` - Daemon 主结构、主循环、子系统组装
- `server/` - UnixSocketServer, Router, JSON-RPC 处理
- `main.rs` - 二进制入口
- `config.rs` - 仅保留 DaemonServerConfig、DaemonConfig 等守护进程专属配置

### Cargo.toml

```toml
[package]
name = "memrecd"
version = "0.6.0-dev"
edition = "2021"

[[bin]]
name = "memrecd"
path = "src/main.rs"

[lib]
name = "memrecd"
path = "src/lib.rs"

[dependencies]
mr-common = { path = "../mr-common" }
mr-protocol = { path = "../mr-protocol" }
mr-ability = { path = "../mr-ability" }
anyhow.workspace = true
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = "0.3"
async-trait = "0.1"
dirs = "5"

[dev-dependencies]
tempfile = "3"
```

### lib.rs

```rust
pub mod config;
pub mod daemon;
pub mod server;
```

---

## 5. 其他 crate 更新

### memrec CLI

```toml
[dependencies]
mr-common = { path = "../mr-common" }
mr-protocol = { path = "../mr-protocol" }
anyhow.workspace = true
serde_json.workspace = true
uuid.workspace = true
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive", "color"] }
dirs = "5"
```

更新所有 `use memrec_common::` 为 `use mr_common::` 和 `use mr_protocol::`

### mr-install

```toml
[dependencies]
mr-common = { path = "../mr-common" }
```

更新 `use memrec_common::` 为 `use mr_common::`

### workspace Cargo.toml

```toml
[workspace]
members = ["mr-common", "mr-protocol", "mr-ability", "memrecd", "memrec", "mr-install"]
resolver = "2"
```

---

## 实施步骤

1. 创建 `mr-common/` 目录，从 `common/` 移入 types/
2. 创建 `mr-protocol/` 目录，从 `common/` 移入 protocol/
3. 创建 `mr-ability/` 目录，从 `memrecd/` 移入核心能力模块
4. 更新所有 Cargo.toml
5. 更新所有 import 路径
6. 删除 `common/` 目录
7. 运行 `cargo build --release` 验证
8. 运行 `cargo test --release` 验证
9. 运行 `cargo clippy --release` 验证

---

## 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| import 路径大量修改 | 可能遗漏 | 使用 sed/脚本批量替换，编译器检查 |
| 配置类型归属不清 | 配置重复定义 | 明确通用配置 vs daemon 专属配置边界 |
| 测试路径变更 | 测试失败 | 随模块迁移测试文件 |
