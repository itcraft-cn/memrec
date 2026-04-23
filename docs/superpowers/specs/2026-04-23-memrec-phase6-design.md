# MemRec Phase 6: 项目记忆隔离 + 语义检索

## 核心原则：AI-First设计

**MemRec是面向AI Agent的工具，而非人类用户。**

设计决策：
1. **命令简洁** - 减少参数冗余，AI不需要交互式提示
2. **输出结构化** - 默认JSON格式，便于AI解析（可选`--human`切换人类可读）
3. **Skill为主入口** - AI通过skill调用，而非阅读CLI文档
4. **自动化优先** - 项目检测、记忆分类尽可能自动化

---

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

## 4. CLI命令设计（AI-First）

### 4.1 设计原则

| 原则 | 说明 |
|------|------|
| **默认JSON** | 所有命令默认输出JSON，便于AI解析 |
| **`--human`可选** | 仅在需要人工检查时使用人类可读格式 |
| **参数简化** | 减少必填参数，利用自动检测 |
| **语义优先** | `search`是核心检索命令，`list`退居次要 |

### 4.2 命令一览

| 命令 | 用途 | 输出格式 |
|------|------|---------|
| `memrec add` | 存储记忆 | JSON (memory_id) |
| `memrec search` | **核心检索** | JSON (results[]) |
| `memrec get` | 获取记忆详情 | JSON (memory) |
| `memrec list` | 列表枚举 | JSON (memories[]) |
| `memrec stats` | 统计信息 | JSON (stats) |
| `memrec project` | 项目管理 | JSON (project_info) |

### 4.3 核心命令详解

#### `memrec add` - 存储记忆

```bash
# 项目记忆（自动检测项目ID）
memrec add "选择JWT认证方案" --mtype decision --tag auth

# 公共记忆
memrec add "用户偏好详细输出" --mtype preference --global

# JSON输出
{"memory_id": "550e8400-e29b-41d4-a716-446655440000", "project_id": "...", "chunk_count": 1}
```

#### `memrec search` - 语义检索（核心）

```bash
# 默认：项目+公共，返回JSON
memrec search "认证方案"

# 指定范围
memrec search "Rust最佳实践" --global-only
memrec search "项目配置" --project-only

# 指定类型
memrec search "决策记录" --mtype decision

# 返回数量
memrec search "用户偏好" -k 20

# JSON输出
{
  "results": [
    {
      "memory_id": "...",
      "score": 0.92,
      "memory_type": "decision",
      "content": "选择JWT+OAuth2认证方案...",
      "project_id": "abc-123",
      "tags": ["auth", "critical"],
      "is_chunked": false,
      "created_at": "2026-04-23T10:30:00Z"
    }
  ],
  "total": 3,
  "search_time_ms": 15
}
```

#### `memrec get` - 获取详情

```bash
# 单个记忆
memrec get <memory_id>

# 合并分块（自动检测chunk_group_id）
memrec get <memory_id> --merge

# JSON输出
{
  "id": "...",
  "memory_type": "decision",
  "content": "完整内容...",
  "merged_from": ["id1", "id2", "id3"],  // 仅--merge时
  "project_id": "...",
  "tags": [...],
  "importance": 0.85,
  "created_at": "...",
  "access_count": 5
}
```

#### `memrec list` - 枚举列表

```bash
# 默认：项目+公共
memrec list -k 50

# 仅公共
memrec list --global-only

# JSON输出
{
  "memories": [...],
  "total": 50,
  "project_count": 35,
  "global_count": 15
}
```

#### `memrec project` - 项目管理

```bash
# 当前项目信息（自动检测）
memrec project

# 所有项目
memrec project --list

# JSON输出
{
  "project_id": "550e8400-...",
  "project_name": "my-project",
  "project_root": "/path/to/project",
  "memory_count": 25,
  ".mr_pid_exists": true
}
```

### 4.4 输出格式切换

```bash
# 默认JSON（AI使用）
memrec search "认证"

# 人类可读（调试用）
memrec search "认证" --human

# 人类可读输出示例
Found 3 memories:

[Decision] 选择JWT+OAuth2认证方案 (score: 0.92)
  选择JWT认证方案用于API保护...
  Tags: auth, critical

[Knowledge] OAuth2最佳实践 (score: 0.85)
  OAuth2授权码流程...
  Tags: oauth, best-practice
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

## 7. Skill更新

### 7.1 Skill文件位置

```
~/.opencode/skills/memrec/SKILL.md
~/.claude/skills/memrec/SKILL.md
```

### 7.2 Skill更新内容

Phase 6后skill需要更新：

**新增触发场景：**
```markdown
触发场景：
1. 重要决策需记录 → memrec add --mtype decision --tag critical
2. 关键知识需保存 → memrec add --mtype knowledge
3. 项目上下文需跨会话保持 → 自动关联项目ID
4. 用户偏好需记忆 → memrec add --mtype preference --global
5. 检索历史知识辅助当前任务 → memrec search "关键词"
```

**新增命令说明：**
```markdown
## 核心命令

### 存储记忆
memrec add "内容" --mtype <type> [--tag <tag>] [--global]

记忆类型：
- decision - 关键决策（推荐--tag critical）
- knowledge - 知识点/最佳实践
- context - 项目配置/环境信息
- preference - 用户偏好（推荐--global）

### 语义检索
memrec search "关键词" [--mtype <type>] [--global-only] [-k <num>]

返回JSON：
{
  "results": [{"memory_id": "...", "score": 0.92, "content": "..."}],
  "total": N
}

### 获取详情
memrec get <memory_id> [--merge]  # --merge用于合并分块

### 项目信息
memrec project  # 当前项目ID和统计
```

### 7.3 Skill完整更新文档

见 `docs/skills/memrec-skill-phase6.md`（实现时创建）

## 8. 部署要求

### 8.1 系统要求

| 要求 | 说明 |
|------|------|
| OS | Linux (推荐), macOS |
| 内存 | ≥512MB (Qdrant嵌入式约200MB) |
| 存储 | ≥500MB (模型+索引) |
| Rust | ≥1.70 |

### 8.2 依赖安装

```bash
# 构建依赖
cargo build --release

# 模型下载（首次运行自动下载）
# all-MiniLM-L6-v2: ~90MB
# 存储位置: ~/.memrec/models/
```

### 8.3 安装步骤

```bash
# 1. 构建
cargo build --release

# 2. 安装二进制
install -m 755 target/release/memrecd ~/.local/bin/
install -m 755 target/release/memrec ~/.local/bin/

# 3. 启动守护进程
./scripts/start.sh
# 或Systemd
./scripts/systemd/install.sh
systemctl --user start memrecd

# 4. 验证
memrec stats
memrec search "test"  # 验证语义检索

# 5. 更新Skill
cp docs/skills/memrec-skill-phase6.md ~/.opencode/skills/memrec/SKILL.md
```

### 8.4 升级现有安装

```bash
# 停止服务
./scripts/stop.sh

# 备份数据
cp -r ~/.memrec ~/.memrec.backup

# 安装新版本
cargo build --release
install -m 755 target/release/memrecd ~/.local/bin/
install -m 755 target/release/memrec ~/.local/bin/

# 数据迁移
memrec migrate --generate-embeddings
memrec migrate --rebuild-index

# 启动服务
./scripts/start.sh
```

## 9. 使用方法（面向AI）

### 9.1 工作流集成

**开始工作时：**
```bash
# 检索相关记忆
memrec search "项目配置" --project-only

# 查看项目信息
memrec project
```

**工作中：**
```bash
# 记录决策
memrec add "选择SQLite替代MySQL" --mtype decision --tag critical

# 记录知识
memrec add "Tokio runtime最佳实践" --mtype knowledge

# 记录用户偏好（公共）
memrec add "用户偏好英文输出" --mtype preference --global

# 检索历史知识
memrec search "认证方案"
```

**工作结束时：**
```bash
# 查看统计
memrec stats
```

### 9.2 AI调用示例

```
# AI Agent执行流程

1. 开始任务 → memrec search "相关历史"
   → {"results": [...]} → 加载历史上下文

2. 做出决策 → memrec add "决策内容" --mtype decision --tag critical
   → {"memory_id": "..."} → 记录成功

3. 需要知识 → memrec search "最佳实践"
   → {"results": [...]} → 获取知识

4. 用户偏好 → memrec add "偏好内容" --mtype preference --global
   → 记录公共偏好

5. 长文本 → 自动拆分，返回 {"chunk_count": 3}
   → 检索时 --merge 获取完整内容
```

### 9.3 `.mr_pid` 处理

AI不需要手动处理`.mr_pid`：
- 自动检测：在项目目录执行命令时自动关联
- 新项目：自动生成`.mr_pid`
- 跨项目：`--global`标记公共记忆

建议将`.mr_pid`加入`.gitignore`（可选，便于团队共享项目ID）。

## 10. 数据迁移

### 10.1 现有记忆处理

Phase 6 发布后，现有记忆自动处理：
1. `project_id = nil` → 视为公共记忆
2. `embedding = None` → 启动时批量生成
3. 分块标签 → 自动提取为 `chunk_*` 字段

### 10.2 迁移命令

```bash
# 生成缺失的embedding（守护进程启动时自动执行）
memrec migrate --embeddings

# 重建Qdrant索引
memrec migrate --rebuild

# 检查迁移状态
memrec migrate --status
```

## 11. 实现计划

### 11.1 分阶段实施

| 阶段 | 内容 | 关键交付 |
|------|------|---------|
| **Phase 6.1** | 项目ID检测 + `.mr_pid` | `detect_project_id()`, 自动生成文件 |
| **Phase 6.2** | Memory结构变更 + 分块关联 | 新字段, 数据迁移 |
| **Phase 6.3** | Qdrant集成 + VectorStore重写 | `qdrant-client`集成, 嵌入式存储 |
| **Phase 6.4** | Embedding生成 + 语义检索 | `search`命令, JSON输出 |
| **Phase 6.5** | CLI命令简化 + JSON输出 | `--human`参数, 输出格式 |
| **Phase 6.6** | Skill更新 + 文档 | SKILL.md更新, 部署文档 |

### 11.2 依赖关系

```
Phase 6.1 ──→ Phase 6.2 ──→ Phase 6.3 ──→ Phase 6.4
                                           ↓
                                    Phase 6.5 ──→ Phase 6.6
```

### 11.3 预计时间

| 阶段 | 时间 | 说明 |
|------|------|------|
| Phase 6.1 | 0.5天 | 项目检测逻辑简单 |
| Phase 6.2 | 0.5天 | 结构变更+迁移 |
| Phase 6.3 | 2天 | Qdrant集成复杂 |
| Phase 6.4 | 1天 | Embedding生成 |
| Phase 6.5 | 0.5天 | CLI调整 |
| Phase 6.6 | 0.5天 | 文档更新 |

**总计：约5天**

---

*Created: 2026-04-23*
*Status: Design Approved*
*Core Principle: AI-First Design*