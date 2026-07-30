# mr-ability

MemRec 核心能力库 - 提供记忆存储、语义搜索、项目隔离等基础服务。

## 概述

`mr-ability` 是 MemRec 生态的核心组件，实现了 AI 记忆跨会话持久化的核心能力。主要提供：

- **多层存储**: 基于 RocksDB 的持久化存储，支持向量、图、全文检索
- **语义搜索**: 混合搜索，结合向量相似度、BM25 和图遍历
- **记忆压缩**: 通过 Dream 处理自动整合旧记忆
- **项目隔离**: 基于 `.mr_pid` 文件的工作区记忆分离
- **嵌入生成**: 文本向量化，用于语义搜索

## 架构

```
mr-ability/
├── storage/      # 多层存储（RocksDB + Tantivy + 向量）
├── search/       # 搜索算法（MMR、评分）
├── embedding/    # 文本嵌入生成
├── dream/        # 记忆整合
├── project/      # 项目检测与隔离
├── tiered/       # 分层搜索系统
├── importance/   # 记忆重要性评估
├── lifecycle/    # 记忆生命周期管理
├── facet/        # 分面搜索
├── edge/         # 图边管理
├── propagation/  # 记忆传播
├── archive/      # 记忆归档
└── rule/         # 规则引擎
```

## 核心组件

### 存储层

存储层提供三个维度的数据持久化：

| 存储 | 说明 | 后端 |
|------|------|------|
| `MemoryStore` | 记忆 CRUD 操作 | RocksDB |
| `VectorStore` | 嵌入向量存储与检索 | RocksDB |
| `EdgeStore` | 图边与遍历 | RocksDB |
| `TantivyStore` | 全文搜索 | Tantivy |
| `HybridStore` | 向量 + BM25 组合 | Vector + Tantivy |

### 搜索系统

支持多种搜索策略：

```rust
use mr_ability::search::{mmr_rerank, MmrConfig};

// MMR 重排优化多样性
let hits = mmr_rerank(&candidates, query_embedding, MmrConfig {
    lambda: 0.5,
    top_k: 10,
});
```

### 嵌入生成

```rust
use mr_ability::embedding::{EmbeddingGenerator, GeneratorFactory};

let generator = GeneratorFactory::create(config)?;
let embedding = generator.embed("重要决策: 使用 Rust")?;
```

### Dream 处理

```rust
use mr_ability::dream::{DreamProcessor, DreamGate};

// 检查是否应执行 Dream
let gate = DreamGate::new(storage, config);
if gate.should_run()? {
    let processor = DreamProcessor::new(llm_client);
    let result = processor.consolidate(old_memories)?;
}
```

### 项目检测

```rust
use mr_ability::project::detect_project_id;

// 从当前目录自动检测项目
let project_id = detect_project_id("/path/to/workspace")?;
```

## 使用方法

添加到 `Cargo.toml`:

```toml
[dependencies]
mr-ability = { path = "../mr-ability" }
```

基础示例：

```rust
use mr_ability::storage::{MemoryStore, RocksDBStore};
use mr_common::Memory;

// 初始化存储
let store = RocksDBStore::open("/path/to/db")?;
let memory_store = MemoryStore::new(store);

// 添加记忆
let memory = Memory::new(
    "重要决策".to_string(),
    MemoryType::Decision,
);
let id = memory_store.add(&memory)?;

// 搜索记忆
let hits = memory_store.search("决策", 10)?;
```

## 存储结构

存储使用 RocksDB 列族组织数据：

| 列族 | 内容 |
|------|------|
| `memories` | 记忆记录（JSON） |
| `vectors` | 嵌入向量（f32 数组） |
| `edges` | 图边（源 → 目标） |
| `facets` | 元数据分面 |
| `rules` | 自动化规则 |
| `config` | 配置键值对 |

## 性能指标

- **向量搜索**: 100K 向量约 10ms（类 HNSW 性能）
- **混合搜索**: 向量 + BM25 + 图组合约 50ms
- **记忆添加**: 含嵌入生成约 5ms
- **Dream**: 整合 10 条记忆约 2s（依赖 LLM）

## 依赖项

- **RocksDB**: 持久化键值存储
- **Tantivy**: 全文搜索引擎
- **FastEmbed**: 基于 ONNX 的嵌入推理
- **SQLite**: 元数据索引
- **Tokio**: 异步运行时

## 许可证

Apache-2.0