# MemRec 使用手册

## 概述

MemRec 是面向 AI CLI 工具的本地记忆持久化系统。它为 AI Agent 提供跨会话的记忆恢复、知识积累、对话存档能力，支持项目隔离和语义检索。

**核心理念：** AI-first — 默认 JSON 输出，命令简洁，为 Agent 而非人类设计。

## 命令一览

| 命令 | 用途 |
|------|------|
| `memrec add` | 添加记忆 |
| `memrec search` | 语义检索 |
| `memrec get` | 获取单条记忆 |
| `memrec list` | 列出记忆 |
| `memrec delete` | 删除记忆 |
| `memrec stats` | 统计信息 |
| `memrec version` | 版本信息 |

## 添加记忆

### 基本语法

```bash
memrec add "内容" --mtype <类型> [--tag <标签>] [--global]
```

### 记忆类型

| 类型 | 标识 | 用途 | 推荐标签 |
|------|------|------|----------|
| 决策 | `decision` | 关键技术/业务决策 | `--tag critical` |
| 知识 | `knowledge` | 知识点、最佳实践、事实 | 见下方细分 |
| 上下文 | `context` | 项目配置、环境信息 | 项目相关标签 |
| 偏好 | `preference` | 用户偏好 | `--global` |
| 对话 | `conversation` | 对话记录（默认） | - |

### knowledge 通过 tag 细分

| tag | 用途 | 示例 |
|-----|------|------|
| `fact` | 物理定律、数学公式、客观事实 | `--tag fact --tag physics` |
| `best-practice` | 最佳实践、设计模式 | `--tag best-practice` |
| `algorithm` | 算法、公式推导 | `--tag algorithm` |
| `tool` | 工具使用技巧 | `--tag tool --tag rust` |

### 示例

```bash
# 决策
memrec add "选择JWT认证方案，理由：无状态、易扩展" --mtype decision --tag auth --tag critical

# 事实类知识
memrec add "光速c=3×10⁸m/s，真空环境中恒定" --mtype knowledge --tag fact --tag physics
memrec add "欧拉公式：e^(iπ)+1=0" --mtype knowledge --tag fact --tag math

# 最佳实践
memrec add "RAII模式：资源获取即初始化，析构自动释放" --mtype knowledge --tag best-practice --tag rust

# 项目上下文
memrec add "技术栈：Rust+Tokio+RocksDB，通信：Unix Socket" --mtype context --tag tech

# 用户偏好（公共记忆）
memrec add "偏好详细输出，不喜好简略模式" --mtype preference --tag output --global
```

### 公共记忆 vs 项目记忆

```bash
# 项目记忆（默认）：仅当前项目可检索
memrec add "项目A的数据库选型" --mtype decision --tag critical

# 公共记忆：所有项目可检索
memrec add "用户偏好暗色主题" --mtype preference --tag ui --global
```

### 长文本自动拆分

超过 7.5KB 的内容会自动拆分为多个 chunk：

```bash
memrec add "很长的内容..." --mtype knowledge
# WARN: Content too long (12.5KB > 7.5KB), auto-splitting into chunks...
# WARN: Split into 2 parts
# Part 1: Added 550e8400-...
# Part 2: Added 6ba7b810-...
# All 2 parts added: [550e8400-..., 6ba7b810-...]
```

每个 chunk 独立生成 embedding，共享 `chunk_group_id`。获取完整内容使用 `--merge`。

## 语义检索

### 基本语法

```bash
memrec search "查询" [选项]
```

### 搜索范围

| 选项 | 范围 | 用途 |
|------|------|------|
| 默认 | 当前项目 + 公共记忆 | 日常使用 |
| `--project-only` | 仅当前项目 | 精确项目内搜索 |
| `--global-only` | 仅公共记忆 | 查找用户偏好 |
| `--all` | 所有项目 | 跨项目关联搜索 |

### 选项

| 选项 | 说明 | 默认值 |
|------|------|--------|
| `-k, --top-k` | 返回结果数 | 10 |
| `--min-score` | 最低相似度阈值 | 0.75（可通过 `MEMREC_MIN_SCORE` 调整） |
| `--project-only` | 仅当前项目 | - |
| `--global-only` | 仅公共记忆 | - |
| `--all` | 跨所有项目 | - |
| `--mtype` | 按类型过滤 | - |
| `--human` | 人类可读输出 | - |

### 示例

```bash
# 基本搜索
memrec search "认证方案"

# 项目内精确搜索
memrec search "性能优化" --project-only

# 公共记忆搜索（用户偏好等）
memrec search "用户偏好" --global-only

# 跨项目搜索（发现关联知识）
memrec search "xlsb" --all

# 调整返回数量和阈值
memrec search "架构" -k 20 --min-score 0.6

# 按类型过滤
memrec search "决策" --mtype decision

# 人类可读格式
memrec search "架构" --human
```

### 相似度评分

| score 范围 | 含义 |
|-----------|------|
| 0.9+ | 高度相关，精确匹配 |
| 0.8-0.9 | 相关，语义相近 |
| 0.75-0.8 | 基本相关 |
| < 0.75 | 可能不相关（被默认过滤） |

**调整阈值：**

```bash
# 临时调整
memrec search "query" --min-score 0.6

# 全局调整
export MEMREC_MIN_SCORE=0.6
memrec search "query"
```

### 输出格式

**默认 JSON（AI Agent 友好）：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "type": "semantic_search_result",
    "results": [
      {
        "memory_id": "550e8400-...",
        "score": 0.86,
        "memory_type": "decision",
        "content_preview": "选择JWT认证方案...",
        "project_id": "312a9769-...",
        "tags": ["auth", "critical"],
        "created_at": "2026-04-23T..."
      }
    ],
    "total": 5,
    "query_embedding_time_ms": 2,
    "search_time_ms": 0
  }
}
```

**人类可读格式（`--human`）：**

```
Found 5 memories:

[DECISION] 选择JWT认证方案... (score: 0.86)
  ID: 550e8400-...
  Project: 312a9769-...
  Tags: ["auth", "critical"]
  Created: 2026-04-23
```

## 获取记忆详情

```bash
# 获取单条记忆
memrec get <memory-id>

# 获取分块记忆的完整内容
memrec get <memory-id> --merge
```

## 列出记忆

```bash
# 默认列出20条
memrec list

# 列出50条
memrec list --limit 50

# 仅当前项目
memrec list --project-only

# 仅公共记忆
memrec list --global-only
```

## 删除记忆

```bash
memrec delete <memory-id>
```

删除为软删除，记忆标记为 `is_deleted=true`。

## 项目隔离

### 自动检测机制

MemRec 自动检测项目上下文，无需手动指定：

1. **git 仓库**：自动检测 git root（`git rev-parse --show-toplevel`）
2. **非 git 目录**：使用当前工作目录
3. **项目标识**：在项目根目录创建 `.mr_pid` 文件

### .mr_pid 文件

自动创建，无需手动管理：

```
your-project/
├── .mr_pid               # 项目ID文件
├── .gitignore            # 建议添加 .mr_pid
└── ...
```

内容示例：

```
memrec_project_id=b435a636-481b-43dd-a819-cc2cedebf365
created_at=2026-04-23T02:45:47.828915366+00:00
```

**注意：**

- 将 `.mr_pid` 添加到 `.gitignore`，避免多人协作时项目ID冲突
- 移动项目目录后，project_id 不变（`.mr_pid` 随项目移动）
- 同一 git 仓库内所有子目录共享同一 project_id

### 项目隔离示例

```bash
# 在 memrec 项目
cd /disk2/code/rust/memrec
memrec add "memrec架构：Rust+RocksDB+UnixSocket" --mtype context
# → 写入 memrec/.mr_pid 的 project_id

# 在 hydrakiller 项目
cd /disk2/code/java/hydrakiller
memrec add "hydrakiller技术栈：Kotlin+Spring+ONNX" --mtype context
# → 写入 hydrakiller/.mr_pid 的 project_id（不同）

# 搜索自动隔离
cd /disk2/code/rust/memrec
memrec search "架构" --project-only  # 仅返回 memrec 项目

# 跨项目搜索
memrec search "架构" --all            # 返回所有项目的架构相关记忆
```

## 工作流集成

### AI Agent 典型工作流

```bash
# 1. 开始任务：检索相关历史
memrec search "相关主题" --project-only

# 2. 做出决策后：记录
memrec add "选择XXX方案，理由：..." --mtype decision --tag critical

# 3. 用户表达偏好：记录为公共记忆
memrec add "用户偏好YYY" --mtype preference --global

# 4. 发现跨项目关联
memrec search "相关主题" --all

# 5. 结束任务
memrec stats
```

### Skill 集成

AI CLI 工具（如 opencode）可通过 Skill 集成：

- Skill 文件：`~/.opencode/skills/memrec/SKILL.md`
- AI agent 自动读取 Skill，了解命令用法和最佳实践

## 性能参考

| 指标 | 数值 |
|------|------|
| 启动时间 | < 50ms |
| 请求延迟 | < 1ms（Unix Socket 本地通信） |
| 搜索延迟 | < 5ms（含 embedding 计算） |
| 内存占用 | ~118MB（含模型） |
| 向量维度 | 384（all-MiniLM-L6-v2） |
