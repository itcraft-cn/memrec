# MemRec - AI CLI 记忆持久化系统设计文档

## 1. 概述

### 1.1 背景

AI CLI工具（如opencode）在长时间交互时存在记忆无法持久的问题。每次新会话都从空白状态开始，无法：
- 恢复之前的对话上下文
- 积累项目知识和最佳实践
- 记住用户偏好和历史决策

### 1.2 目标

构建一个本地化的记忆持久化系统，提供：
- 跨会话记忆恢复
- 知识库积累
- 对话历史存档
- 智能记忆管理（压缩与遗忘）

### 1.3 范围

**第一期范围：**
- memrecd: 守护进程服务（Unix Socket通信）
- memrec: CLI客户端工具
- 核心存储和检索功能
- 智能记忆生命周期管理

**后续迭代：**
- HTTP API支持
- MCP协议集成

---

## 2. 系统架构

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────┐
│                   AI CLI (opencode)                  │
└────────────────────────┬────────────────────────────┘
                         │
                 ┌───────┴────────┐
                 │   memrec-cli  │
                 │   (memrec)    │
                 └───────┬────────┘
                         │ Unix Socket
                 ┌───────┴────────┐
                 │  memrecd       │
                 │  (Daemon)      │
                 └───────┬────────┘
                         │
            ┌────────────┼────────────┐
            │            │            │
     ┌──────┴─────┐ ┌───┴──────┐ ┌────┴──────┐
     │  RocksDB   │ │  usearch │ │  Memory   │
     │  (KV存储)  │ │ (向量库) │ │ Manager   │
     └────────────┘ └──────────┘ └───────────┘
```

### 2.2 组件职责

**memrecd (守护进程)**
- Unix Socket监听，处理客户端请求
- 存储管理（RocksDB + usearch）
- 记忆生命周期管理
- 向量嵌入生成（调用本地或远程嵌入服务）

**memrec (CLI工具)**
- 用户交互界面
- 与memrecd通信
- 结果格式化展示

---

## 3. 数据模型

### 3.1 记忆模型

```rust
struct Memory {
    id: Uuid,                              // 唯一标识
    memory_type: MemoryType,               // 记忆类型
    content: String,                       // 内容
    summary: Option<String>,               // 压缩后的摘要
    embedding: Option<Vec<f32>>,           // 向量嵌入
    importance: f32,                       // 重要性评分 [0.0, 1.0]
    created_at: DateTime<Utc>,             // 创建时间
    last_accessed: DateTime<Utc>,          // 最后访问时间
    access_count: u32,                     // 访问次数
    tags: Vec<String>,                     // 标签
    metadata: HashMap<String, String>,      // 元数据
    project_id: Option<Uuid>,              // 所属项目
    is_deleted: bool,                      // 软删除标记
    deleted_at: Option<DateTime<Utc>>,      // 删除时间
}

enum MemoryType {
    Conversation,  // 对话记录
    Knowledge,     // 知识点
    Decision,      // 决策记录
    Preference,    // 用户偏好
    Context,       // 项目上下文
}
```

### 3.2 项目模型

```rust
struct Project {
    id: Uuid,
    name: String,
    description: Option<String>,
    created_at: DateTime<Utc>,
    config: ProjectConfig,
}

struct ProjectConfig {
    memory_config: MemoryConfig,  // 项目级记忆配置
    active: bool,                  // 是否为当前项目
}
```

### 3.3 配置模型

```rust
struct MemoryConfig {
    // 遗忘策略
    soft_delete_recovery_days: u32,    // 软删除恢复期（默认30天）
    hard_delete_importance: f32,      // 硬删除重要性阈值（默认0.1）
    hard_delete_inactive_days: u32,    // 硬删除未访问天数（默认90天）
    
    // 压缩策略
    compression_importance: f32,      // 压缩触发重要性（默认0.3）
    
    // 内存池
    max_storage_gb: usize,            // 最大存储（默认10GB）
    high_watermark: f32,              // 高水位线（默认0.9）
    low_watermark: f32,               // 低水位线（默认0.7）
}
```

---

## 4. 存储设计

### 4.1 RocksDB存储结构

**Column Families:**

| CF名称 | Key | Value | 用途 |
|--------|-----|-------|------|
| `memories` | `memory_id` | Memory JSON | 记忆主存储 |
| `by_tag` | `tag:memory_id` | `memory_id` | 标签索引 |
| `by_project` | `project_id:memory_id` | `memory_id` | 项目索引 |
| `by_time` | `timestamp:memory_id` | `memory_id` | 时间索引 |
| `importance` | `memory_id` | importance score | 重要性索引 |
| `projects` | `project_id` | Project JSON | 项目存储 |
| `config` | `config_key` | config JSON | 配置存储 |

### 4.2 usearch向量存储

```
向量索引结构：
- Dimension: 1536 (OpenAI embedding) 或自定义
- Metric: Cosine Similarity
- Index Type: HNSW (Hierarchical Navigable Small World)

内存映射：
memory_id <-> vector_id (一对一映射)
```

### 4.3 混合索引策略

```
写入流程：
1. Memory → RocksDB (主存储)
2. Memory.tags → by_tag CF (标签索引)
3. Memory.embedding → usearch (向量索引)
4. Memory.importance → importance CF (重要性索引)

查询流程：
精确查询 → RocksDB CF查询
语义查询 → usearch向量检索
混合查询 → 结果融合 + 重排序
```

---

## 5. 记忆生命周期管理

### 5.1 生命周期阶段

```
新记忆 → 活跃期 → 衰减期 → 压缩/归档 → 遗忘
         (高频访问)  (低频访问)  (摘要保存)  (删除)
```

### 5.2 重要性评分算法

```rust
fn calculate_importance(memory: &Memory, config: &MemoryConfig) -> f32 {
    let now = Utc::now();
    
    // 时间衰减因子 (指数衰减)
    let days_since_access = (now - memory.last_accessed).num_days() as f32;
    let recency = (-0.05 * days_since_access).exp();
    
    // 访问频率因子 (对数增长)
    let frequency = (memory.access_count as f32 + 1.0).ln() / 10.0;
    
    // 语义重要性 (基于标签权重)
    let relevance = semantic_importance(&memory.tags);
    
    // 用户显式优先级
    let explicit = memory.metadata.get("priority")
        .and_then(|p| p.parse::<f32>().ok())
        .unwrap_or(0.5);
    
    // 加权融合
    0.3 * recency + 0.2 * frequency + 0.2 * relevance + 0.3 * explicit
}
```

### 5.3 压缩策略

**触发条件：**
- 存储达到高水位线（默认90%）
- 记忆重要性 < 0.3

**压缩算法：**
```rust
// 对话压缩：多轮对话 → 摘要 + 关键决策点
fn compress_conversation(memories: Vec<Memory>) -> Memory {
    // 1. 提取关键信息（决策、结论、重要引用）
    // 2. 生成摘要（使用LLM）
    // 3. 创建压缩后的记忆节点
    // 4. 保留原始记忆ID引用（可追溯）
}

// 知识压缩：相似知识 → 合并去重
fn compress_knowledge(memories: Vec<Memory>) -> Vec<Memory> {
    // 1. 向量聚类
    // 2. 识别重复或高度相似的知识
    // 3. 合并内容，保留最完整版本
    // 4. 更新标签和引用
}
```

### 5.4 遗忘策略

**软删除：**
- 标记 `is_deleted = true`
- 设置 `deleted_at` 时间戳
- 保留30天可恢复

**硬删除条件（满足任一）：**
1. 重要性 < 0.1 且 超过90天未访问
2. 用户显式标记删除 且 已软删除超过恢复期
3. 存储压力极高（达高水位线）且为低优先级记忆

**删除流程：**
```
1. 从RocksDB移除所有索引
2. 从usearch删除向量
3. 记录删除日志（审计用）
```

### 5.5 自动管理流程

```rust
// 定期执行（每小时）
async fn memory_management_cycle() {
    // 1. 重新计算重要性
    recalculate_importance();
    
    // 2. 检查存储水位
    let usage = get_storage_usage();
    
    if usage > config.high_watermark {
        // 3. 触发压缩
        compress_low_importance_memories();
        
        // 4. 触发遗忘
        forget_obsolete_memories();
        
        // 5. 如果仍然超过高水位，紧急清理
        if get_storage_usage() > config.high_watermark {
            emergency_cleanup();
        }
    }
}
```

---

## 6. 检索系统

### 6.1 检索模式

```rust
enum SearchMode {
    Exact,      // 精确匹配：标签、关键词、时间范围
    Semantic,   // 语义检索：向量相似度
    Hybrid,     // 混合检索：融合精确和语义结果
}
```

### 6.2 查询模型

```rust
struct SearchQuery {
    mode: SearchMode,
    text: Option<String>,              // 搜索文本
    tags: Option<Vec<String>>,         // 标签过滤
    time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,  // 时间范围
    project_id: Option<Uuid>,          // 项目过滤
    top_k: usize,                      // 返回数量（默认10）
    min_importance: f32,               // 最小重要性（默认0.0）
    include_deleted: bool,             // 包含已删除（默认false）
}

struct SearchResult {
    memories: Vec<Memory>,
    total: usize,
    mode: SearchMode,
    elapsed_ms: u64,
}
```

### 6.3 混合检索算法

```rust
fn hybrid_search(query: SearchQuery) -> SearchResult {
    let mut results = Vec::new();
    
    // 1. 精确查询
    let exact_results = if has_exact_filters(&query) {
        rocksdb_exact_search(&query)
    } else {
        Vec::new()
    };
    
    // 2. 语义查询
    let semantic_results = if query.text.is_some() {
        let embedding = generate_embedding(query.text.unwrap());
        usearch_semantic_search(&embedding, query.top_k * 2)
    } else {
        Vec::new()
    };
    
    // 3. 结果融合与重排序
    results = merge_and_rerank(exact_results, semantic_results, &query);
    
    // 4. 截断到top_k
    results.truncate(query.top_k);
    
    SearchResult {
        memories: results,
        total: results.len(),
        mode: SearchMode::Hybrid,
        elapsed_ms: /* ... */,
    }
}

fn merge_and_rerank(
    exact: Vec<Memory>,
    semantic: Vec<Memory>,
    query: &SearchQuery,
) -> Vec<Memory> {
    // Reciprocal Rank Fusion (RRF)算法
    let k = 60.0;
    let mut scores: HashMap<Uuid, f32> = HashMap::new();
    
    // 精确结果打分
    for (rank, memory) in exact.iter().enumerate() {
        let score = 1.0 / (k + rank as f32 + 1.0);
        *scores.entry(memory.id).or_insert(0.0) += score * 0.6; // 权重0.6
    }
    
    // 语义结果打分
    for (rank, memory) in semantic.iter().enumerate() {
        let score = 1.0 / (k + rank as f32 + 1.0);
        *scores.entry(memory.id).or_insert(0.0) += score * 0.4; // 权重0.4
    }
    
    // 按分数排序
    let mut results: Vec<Memory> = scores.keys()
        .filter_map(|id| get_memory(id))
        .collect();
    results.sort_by(|a, b| {
        scores[&b.id].partial_cmp(&scores[&a.id]).unwrap()
    });
    
    results
}
```

---

## 7. CLI接口设计

### 7.1 命令列表

**记忆管理：**
```bash
memrec add <content> [--type <type>] [--tag <tag>...] [--project <project>]
  # 添加记忆
  # --type: conversation|knowledge|decision|preference|context (默认conversation)
  # --tag: 可多次使用，添加多个标签
  # --project: 指定项目（默认当前项目）

memrec list [--tag <tag>] [--type <type>] [--limit <n>]
  # 列出记忆
  # --limit: 返回数量（默认20）

memrec get <id>
  # 获取具体记忆

memrec update <id> [--content <content>] [--tag <tag>...]
  # 更新记忆

memrec delete <id> [--force]
  # 删除记忆
  # --force: 硬删除（跳过软删除）

memrec tag <id> <tag>
  # 添加标签

memrec untag <id> <tag>
  # 移除标签
```

**检索：**
```bash
memrec search <query> [options]
  # 混合检索
  # --mode: exact|semantic|hybrid (默认hybrid)
  # --tag <tag>: 标签过滤
  # --after <date>: 时间范围起始
  # --before <date>: 时间范围结束
  # --project <project>: 项目过滤
  # --top-k <n>: 返回数量（默认10）
  # --min-importance <score>: 最小重要性

memrec recall [--limit <n>]
  # 最近访问的记忆
  # --limit: 返回数量（默认10）

memrec similar <id> [--limit <n>]
  # 相似记忆（基于向量）
  # --limit: 返回数量（默认5）
```

**项目管理：**
```bash
memrec project create <name> [--description <desc>]
  # 创建项目

memrec project list
  # 列出项目

memrec project switch <name>
  # 切换项目

memrec project delete <name> [--force]
  # 删除项目
  # --force: 强制删除（包含记忆）
```

**配置：**
```bash
memrec config set <key> <value>
  # 设置配置
  # 例如: memrec config set max_storage_gb 20

memrec config get <key>
  # 获取配置

memrec config show
  # 显示所有配置

memrec config reset
  # 重置为默认配置
```

**维护：**
```bash
memrec stats
  # 存储统计（记忆数量、存储大小、重要性分布等）

memrec compress
  # 手动触发压缩

memrec forget
  # 手动触发遗忘

memrec export <file> [--format <format>]
  # 导出记忆
  # --format: json|bson (默认json)

memrec import <file>
  # 导入记忆

memrec daemon start [--foreground]
  # 启动守护进程
  # --foreground: 前台运行

memrec daemon stop
  # 停止守护进程

memrec daemon status
  # 守护进程状态
```

### 7.2 输出格式

**默认输出：**
```
# memrec list
[2024-01-15 10:30:45] [knowledge] #abc123 如何配置Rust项目...
  Tags: rust, config
  Importance: 0.85

[2024-01-15 09:20:10] [conversation] #def456 讨论了用户认证方案...
  Tags: auth, security
  Importance: 0.72
```

**JSON输出（--json）：**
```json
{
  "memories": [
    {
      "id": "abc123",
      "type": "knowledge",
      "content": "如何配置Rust项目...",
      "importance": 0.85,
      "tags": ["rust", "config"],
      "created_at": "2024-01-15T10:30:45Z"
    }
  ],
  "total": 1
}
```

---

## 8. 通信协议

### 8.1 Unix Socket通信

**Socket路径：** `~/.memrec/memrecd.sock`

**协议：** JSON-RPC 2.0

**请求示例：**
```json
{
  "jsonrpc": "2.0",
  "method": "search",
  "params": {
    "text": "Rust配置",
    "mode": "hybrid",
    "top_k": 10
  },
  "id": 1
}
```

**响应示例：**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "memories": [...],
    "total": 5,
    "elapsed_ms": 23
  },
  "id": 1
}
```

**错误响应：**
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32602,
    "message": "Invalid params",
    "data": "top_k must be positive"
  },
  "id": 1
}
```

### 8.2 BSON支持

**使用场景：** 大数据量传输（如导入导出）

**切换方式：** `Accept: application/bson` 或命令行参数 `--bson`

---

## 9. 技术实现

### 9.1 技术栈

**语言：** Rust

**核心依赖：**
```toml
[dependencies]
# 存储
rocksdb = "0.22"
usearch = "0.25"

# 异步运行时
tokio = { version = "1", features = ["full"] }

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bson = "2"

# 核心类型
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

# CLI
clap = { version = "4", features = ["derive"] }

# 错误处理
anyhow = "1"
thiserror = "1"

# 日志
tracing = "0.1"
tracing-subscriber = "0.3"

# 向量嵌入（可选）
candle = { version = "0.4", optional = true }  # 本地嵌入模型
```

### 9.2 项目结构

```
memrec/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── docs/
│   └── superpowers/
│       └── specs/
│           └── 2026-04-23-memrec-design.md
├── memrecd/                    # 守护进程
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── storage/
│       │   ├── mod.rs
│       │   ├── rocksdb.rs      # RocksDB封装
│       │   └── usearch.rs      # usearch封装
│       ├── manager/
│       │   ├── mod.rs
│       │   ├── memory.rs       # 记忆管理
│       │   ├── lifecycle.rs    # 生命周期管理
│       │   └── compression.rs  # 压缩算法
│       ├── search/
│       │   ├── mod.rs
│       │   ├── exact.rs        # 精确检索
│       │   ├── semantic.rs     # 语义检索
│       │   └── hybrid.rs       # 混合检索
│       ├── server/
│       │   ├── mod.rs
│       │   ├── unix_socket.rs  # Unix Socket服务
│       │   └── protocol.rs     # JSON-RPC协议
│       └── config/
│           ├── mod.rs
│           └── settings.rs     # 配置管理
├── memrec/                     # CLI工具
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── client/
│       │   ├── mod.rs
│       │   └── connection.rs   # 与server通信
│       └── commands/
│           ├── mod.rs
│           ├── memory.rs       # 记忆命令
│           ├── search.rs        # 检索命令
│           ├── project.rs       # 项目命令
│           ├── config.rs        # 配置命令
│           └── daemon.rs        # 守护进程命令
└── common/                     # 共享代码
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── types/
        │   ├── mod.rs
        │   ├── memory.rs        # Memory类型
        │   ├── project.rs       # Project类型
        │   └── config.rs         # Config类型
        └── protocol/
            ├── mod.rs
            ├── request.rs       # 请求类型
            └── response.rs      # 响应类型
```

### 9.3 关键接口定义

**common/src/types/memory.rs:**
```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Conversation,
    Knowledge,
    Decision,
    Preference,
    Context,
}
```

**common/src/protocol/request.rs:**
```rust
use serde::{Deserialize, Serialize};
use crate::types::{MemoryType, Memory};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Request {
    Add {
        content: String,
        memory_type: MemoryType,
        tags: Vec<String>,
        project_id: Option<Uuid>,
    },
    Get {
        id: Uuid,
    },
    Search {
        query: SearchQuery,
    },
    Update {
        id: Uuid,
        content: Option<String>,
        tags: Option<Vec<String>>,
    },
    Delete {
        id: Uuid,
        force: bool,
    },
    // ... 其他操作
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchQuery {
    pub mode: SearchMode,
    pub text: Option<String>,
    pub tags: Option<Vec<String>>,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub project_id: Option<Uuid>,
    pub top_k: usize,
    pub min_importance: f32,
    pub include_deleted: bool,
}
```

---

## 10. 向量嵌入

### 10.1 嵌入模型选择

**第一期方案：调用远程API**
- OpenAI Embedding API (`text-embedding-3-small`)
- 备选：本地运行 sentence-transformers

**后续迭代：本地模型**
- candle-transformers + HuggingFace模型
- 优势：离线可用、隐私保护

### 10.2 嵌入生成流程

```rust
async fn generate_embedding(content: &str) -> Result<Vec<f32>> {
    // 1. 文本预处理（截断、清洗）
    let text = preprocess_text(content);
    
    // 2. 调用嵌入服务
    let embedding = if config.use_local_model {
        local_embedding_model.embed(&text).await?
    } else {
        openai_embedding_api.embed(&text, &config.api_key).await?
    };
    
    Ok(embedding)
}
```

### 10.3 嵌入缓存

```rust
// 使用RocksDB缓存嵌入结果，避免重复计算
struct EmbeddingCache {
    db: RocksDB,  // 专用CF: "embedding_cache"
}

impl EmbeddingCache {
    fn get(&self, text_hash: &str) -> Option<Vec<f32>> {
        self.db.get_cf("embedding_cache", text_hash)
    }
    
    fn set(&self, text_hash: &str, embedding: &[f32]) {
        self.db.put_cf("embedding_cache", text_hash, embedding);
    }
}
```

---

## 11. 配置管理

### 11.1 配置文件

**路径：** `~/.memrec/config.toml`

```toml
[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"

[storage]
max_storage_gb = 10
high_watermark = 0.9
low_watermark = 0.7

[lifecycle]
soft_delete_recovery_days = 30
hard_delete_importance = 0.1
hard_delete_inactive_days = 90
compression_importance = 0.3

[embedding]
provider = "openai"  # openai | local
model = "text-embedding-3-small"
api_key = ""  # 从环境变量 OPENAI_API_KEY 读取
cache_enabled = true

[log]
level = "info"  # trace | debug | info | warn | error
file = "~/.memrec/memrecd.log"
```

### 11.2 配置优先级

1. 命令行参数（最高）
2. 环境变量（`MEMREC_*`）
3. 配置文件
4. 默认值（最低）

---

## 12. 测试策略

### 12.1 单元测试

- 存储层：RocksDB读写、usearch索引
- 管理层：重要性计算、压缩算法
- 检索层：精确查询、语义查询、混合查询

### 12.2 集成测试

- CLI与服务端通信
- Unix Socket协议
- 完整的记忆生命周期

### 12.3 性能测试

- 大数据量写入（10万+记忆）
- 混合检索延迟（< 100ms）
- 内存占用（< 500MB for 10万记忆）

---

## 13. 部署与运维

### 13.1 安装

```bash
# 编译
cargo build --release

# 安装
cargo install --path memrecd
cargo install --path memrec

# 初始化
memrec daemon init
```

### 13.2 启动守护进程

```bash
# 后台启动
memrec daemon start

# 查看状态
memrec daemon status

# 停止
memrec daemon stop
```

### 13.3 Systemd服务（可选）

```ini
[Unit]
Description=MemRec Memory Daemon
After=network.target

[Service]
Type=simple
User=helly
ExecStart=/usr/local/bin/memrecd
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

---

## 14. 安全考虑

### 14.1 本地安全

- Unix Socket权限：仅当前用户可访问
- 数据加密：可选的数据加密（RocksDB加密）
- 敏感信息：API密钥存储在环境变量或加密配置

### 14.2 访问控制

- 本地信任模型：信任本地用户
- 未来扩展：多用户支持、权限管理

---

## 15. 性能指标

### 15.1 目标性能

| 指标 | 目标值 |
|------|--------|
| 写入延迟 | < 10ms (不含嵌入生成) |
| 精确检索延迟 | < 10ms |
| 语义检索延迟 | < 50ms |
| 混合检索延迟 | < 100ms |
| 内存占用 | < 500MB (10万记忆) |
| 磁盘占用 | < 2GB (10万记忆，含向量) |

### 15.2 优化策略

- **写入优化**：批量写入、异步嵌入生成
- **检索优化**：索引优化、查询缓存
- **存储优化**：压缩、去重、向量量化

---

## 16. 后续迭代

### 16.1 第二期

- HTTP API支持
- Web管理界面
- 嵌入模型本地化

### 16.2 第三期

- MCP协议集成
- 多用户支持
- 云端同步（可选）

---

## 17. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 向量嵌入API限流 | 性能下降 | 本地缓存、批量请求、本地模型备选 |
| 存储空间耗尽 | 服务不可用 | 高水位告警、自动清理、用户通知 |
| 数据损坏 | 数据丢失 | 定期备份、WAL日志、数据校验 |
| 性能退化 | 用户体验差 | 性能监控、索引优化、定期维护 |

---

## 18. 开发计划

### 18.1 第一期开发（预计4-6周）

**Week 1-2: 基础设施**
- 项目脚手架
- RocksDB集成
- usearch集成
- 配置管理

**Week 3-4: 核心功能**
- 记忆管理
- Unix Socket服务
- CLI工具
- 检索系统

**Week 5-6: 高级功能与测试**
- 生命周期管理
- 压缩算法
- 测试与优化
- 文档完善

### 18.2 后续迭代

- 第二期：HTTP API、Web界面（预计2-3周）
- 第三期：MCP协议、多用户（预计3-4周）

---

## 附录

### A. 参考资料

- RocksDB: https://rocksdb.org/
- usearch: https://github.com/unum-cloud/usearch
- JSON-RPC 2.0: https://www.jsonrpc.org/specification

### B. 相关项目

- MemGPT: https://github.com/cpacker/MemGPT
- LangChain Memory: https://python.langchain.com/docs/modules/memory/
- ChromaDB: https://www.trychroma.com/