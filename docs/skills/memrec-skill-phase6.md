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

---

*Updated: 2026-04-23*