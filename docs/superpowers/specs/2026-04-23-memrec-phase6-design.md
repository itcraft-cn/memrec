# MemRec Phase 6: 项目记忆隔离 + 语义检索

## 概述

Phase 6 实现三个核心功能：
1. **公共/项目记忆分离** - 通过 `project_id` 区分记忆范围
2. **项目ID自动管理** - `.mr_pid` 文件确保记忆跨目录迁移延续
3. **语义检索** - Qdrant嵌入式存储 + FastEmbed实现向量检索

## 1. 项目记忆隔离

### 1.1 记忆范围定义

| 类型 | `project_id` | 用途 |
|------|-------------|------|
| 公共记忆 | `Uuid::nil()` (全0) | 跨项目通用知识、用户偏好 |
| 项目记忆 | 具体UUID | 特定项目的决策、上下文、配置 |

### 1.2 项目ID文件 `.mr_pid`

**位置：** 项目根目录（Git根目录或当前目录）

**格式：**
```
memrec_project_id=550e8400-e29b-41d4-a716-446655440000
created_at=2026-04-23T10:30:00Z
project_name=my-project
```

**作用：**
- 项目目录移动后记忆延续（UUID不变）
- 非Git项目也能识别项目边界

### 1.3 项目检测逻辑

```
执行memrec命令时:
1. 查找Git根目录 (git rev-parse --show-toplevel)
   - 失败 → 当前目录即为项目根目录
2. 检查 {project_root}/.mr_pid 是否存在
   - 存在 → 读取UUID作为project_id
   - 不存在 → 生成UUID，写入.mr_pid，返回新project_id
3. 存储记忆时自动关联该project_id
```

### 1.4 Memory结构变更

```rust
pub struct Memory {
    // 现有字段...
    pub project_id: Option<Uuid>,       // nil=公共，具体UUID=项目
    
    // 新增：分块关联
    pub chunk_group_id: Option<Uuid>,   // 同一原始文本的分块共享此ID
    pub chunk_index: Option<u32>,       // 分块序号 (0-based)
    pub chunk_total: Option<u32>,       // 总分块数
}
```

### 1.5 检索范围控制

| 命令参数 | 检索范围 |
|---------|---------|
| 默认 | 项目记忆 + 公共记忆 |
| `--project-only` | 仅当前项目记忆 |
| `--global-only` | 仅公共记忆 |
| `--project-id <uuid>` | 指定项目记忆 |

## 2. 分块关联机制

### 2.1 背景

长文本（>7.5KB）自动拆分为小块存储，需要：
1. 每个分块独立生成embedding
2. 检索时能够关联同一原始文本的所有分块
3. 提供 `--merge` 选项获取完整内容

### 2.2 存储示例

```
原始长文本 → 拆分为3块，生成 chunk_group_id = abc-123

Memory 1:
  - id: uuid-1
  - chunk_group_id: abc-123
  - chunk_index: 0
  - chunk_total: 3
  
Memory 2:
  - id: uuid-2
  - chunk_group_id: abc-123
  - chunk_index: 1
  - chunk_total: 3
  
Memory 3:
  - id: uuid-3
  - chunk_group_id: abc-123
  - chunk_index: 2
  - chunk_total: 3
```

### 2.3 检索行为

```bash
$ memrec search "关键词"

[Decision] 匹配内容片段...
  ID: uuid-2 (chunk 1/3)
  Chunk Group: abc-123
  ⚠️  Chunked memory. Use `memrec get abc-123 --merge` for full content.

$ memrec get abc-123 --merge

[Decision] 完整长文本内容...
  (合并了所有3个分块)
  Original IDs: [uuid-1, uuid-2, uuid-3]
```

## 3. Qdrant嵌入式存储

### 3.1 技术选型

| 组件 | 选择 | 说明 |
|------|------|------|
| 向量数据库 | `qdrant-client` (Rust) | 嵌入式模式，数据持久化 |
| Embedding | `fastembed` 或 `candle-transformers` | 本地运行，无需API |
| 模型 | `all-MiniLM-L6-v2` (384维) | 轻量级，CPU友好 |

### 3.2 存储架构

```
~/.memrec/
├── db/                      # RocksDB (Memory元数据)
│   ├── memory/
│   ├── project/
│   └── config/
└── qdrant/                  # Qdrant嵌入式数据
    └── collection/
```

### 3.3 Collection设计

```
Collection: memories
Points:
  - id: memory UUID
  - vector: [f32; 384]  # embedding
  - payload:
    - project_id: UUID | nil
    - memory_type: string
    - tags: [string]
    - content_preview: string (前200字符)
    - importance: f32
    - created_at: timestamp
    - chunk_group_id: UUID | nil
    - chunk_index: u32 | nil
    - chunk_total: u32 | nil
```

### 3.4 Embedding生成策略

| 场景 | 处理 |
|------|------|
| `memrec add` | 同步生成embedding（短文本），异步生成（长文本拆分） |
| 启动时 | 检查未生成embedding的记忆，批量补生成 |
| 检索 | 查询文本实时生成embedding |

## 4. CLI命令变更

### 4.1 命令一览

| 命令 | 变化 |
|------|------|
| `memrec add` | 新增 `--global` 参数 |
| `memrec list` | 新增 `--project-only`, `--global-only` |
| `memrec search` | **新增** 语义检索命令 |
| `memrec get` | 新增 `--merge` 合并分块 |
| `memrec project` | **新增** 项目管理子命令 |

### 4.2 详细命令

#### `memrec add`

```bash
# 项目记忆（默认）
memrec add "选择JWT认证方案" --mtype decision --tag auth

# 公共记忆
memrec add "用户偏好详细输出" --mtype preference --tag output --global

# 强制指定项目ID
memrec add "项目A依赖项目B" --mtype context --project-id <uuid>
```

#### `memrec list`

```bash
memrec list                     # 项目+公共
memrec list --project-only      # 仅项目
memrec list --global-only       # 仅公共
memrec list --project-id <uuid> # 指定项目
```

#### `memrec search` (新增)

```bash
memrec search "用户偏好" --top-k 10
memrec search "认证方案" --project-only
memrec search "设计文档" --merge-chunks
memrec search "决策" --mtype decision
```

#### `memrec get`

```bash
memrec get <id>                 # 单个记忆
memrec get <chunk-group-id> --merge  # 合并分块
```

#### `memrec project` (新增)

```bash
memrec project info             # 当前项目信息
memrec project list             # 所有项目列表
memrec project init --name "my-project"  # 初始化
memrec project prune            # 清理未使用项目
```

### 4.3 输出格式

```
$ memrec search "认证"

Found 3 memories (score >= 0.7):

[Decision] 选择JWT+OAuth2认证方案 (score: 0.92)
  ID: 550e8400-e29b-41d4-a716-446655440000
  Project: my-project
  Tags: ["auth", "security", "critical"]
  Created: 2026-04-23

[Knowledge] OAuth2最佳实践 (score: 0.85)
  ID: ...
  Project: (global)
  ⚠️  Chunked memory (2/5). Use `memrec get xxx --merge`.

[Decision] 放弃Session方案 (score: 0.78)
  ID: ...
  Project: my-project
```

## 5. JSON-RPC协议变更

### 5.1 新增Action

```rust
pub enum RequestAction {
    // 现有
    AddMemory,
    GetMemory,
    ListMemory,
    DeleteMemory,
    GetStats,
    
    // 新增
    SearchMemory,      // 语义检索
    MergeChunks,       // 合并分块
    GetProjectInfo,    // 项目信息
    ListProjects,      // 项目列表
    InitProject,       // 项目初始化
}
```

### 5.2 SearchMemory接口

```rust
// Request
pub struct SearchParams {
    pub query: String,
    pub project_id: Option<Uuid>,
    pub include_global: bool,
    pub memory_type: Option<MemoryType>,
    pub top_k: u32,
    pub min_score: f32,
    pub merge_chunks: bool,
}

// Response
pub struct SearchResult {
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
}

pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: u32,
    pub query_embedding_time_ms: u64,
    pub search_time_ms: u64,
}
```

### 5.3 ProjectInfo接口

```rust
pub struct ProjectInfo {
    pub project_id: Uuid,
    pub project_name: String,
    pub project_root: PathBuf,
    pub memory_count: u32,
    pub last_activity: DateTime<Utc>,
}
```

## 6. 依赖变更

### 6.1 新增依赖

```toml
# Cargo.toml (workspace)

[workspace.dependencies]
qdrant-client = "1.7"        # Qdrant Rust客户端
fastembed = { version = "3", optional = true }  # FastEmbed (可选)
candle-transformers = { version = "0.4", optional = true }  # Candle (可选)
```

### 6.2 依赖说明

| Crate | 用途 |
|-------|------|
| `qdrant-client` | 嵌入式Qdrant，向量存储和检索 |
| `fastembed` | 快速embedding生成（推荐） |
| `candle-transformers` | 本地模型推理（备选） |

## 7. 数据迁移

### 7.1 现有记忆处理

Phase 6 发布后，现有记忆自动处理：
1. `project_id = nil` → 视为公共记忆
2. `embedding = None` → 启动时批量生成
3. 分块标签 → 自动提取为 `chunk_*` 字段

### 7.2 迁移脚本

```bash
# 生成缺失的embedding
memrec migrate --generate-embeddings

# 重建Qdrant索引
memrec migrate --rebuild-index

# 旧分块标签转新字段
memrec migrate --fix-chunks
```

## 8. 实现计划

### Phase 6 分阶段

| 阶段 | 内容 | 预计时间 |
|------|------|---------|
| Phase 6.1 | 项目ID检测 + `.mr_pid` | 1天 |
| Phase 6.2 | Memory结构变更 + 分块关联 | 1天 |
| Phase 6.3 | Qdrant集成 + VectorStore重写 | 2天 |
| Phase 6.4 | Embedding生成 + 语义检索 | 2天 |
| Phase 6.5 | CLI命令变更 + 测试 | 1天 |
| Phase 6.6 | 数据迁移 + 文档 | 1天 |

**总计：约8天**

---

*Created: 2026-04-23*
*Status: Design Approved*