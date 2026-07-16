# MemRec 使用手册

## 概述

MemRec 是面向 AI CLI 工具的本地优先记忆持久化系统。它为 AI Agent 提供跨会话的记忆恢复、知识积累、对话存档能力，支持项目隔离和语义检索。

**核心理念：** AI-first — 默认 JSON 输出，命令简洁，为 Agent 而非人类设计。

## 安装

### 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux / macOS |
| Rust | 1.75+（仅 mr-install 首次安装需要） |
| 磁盘空间 | ~200MB（MiniLM）/ ~2.5GB（BGE-M3，含模型） |
| 内存 | ~118MB（MiniLM）/ ~1.5GB（BGE-M3）运行时 |

### 一键安装

```bash
cargo install --locked mr-install
mr-install
```

`mr-install` 自动完成：
1. 通过 `cargo install` 安装 memrec/memrecd
2. 创建 `~/.memrec/` 目录结构
3. 下载 ONNX Embedding 模型
4. 注册并启动守护进程服务
5. 验证安装

### 选择 Embedding 模型

| 模型 | 维度 | 适用场景 | 磁盘空间 | 内存 | 默认 min_score |
|------|------|----------|----------|------|----------------|
| `minilm-l6-v2`（默认） | 384 | 纯英文 | ~90MB | ~118MB | 0.75 |
| `bge-m3` | 1024 | 中文/多语言 | ~2.3GB | ~1.5GB | 0.5 |

```bash
# 默认：MiniLM-L6-v2（英文）
mr-install

# BGE-M3（中文/多语言，中文用户推荐）
mr-install --model bge-m3
```

**为什么 min_score 默认值不同？** BGE-M3 的余弦相似度分数天然低于 MiniLM。MiniLM 精确匹配 >0.8；BGE-M3 精确匹配约 0.74。BGE-M3 的 0.5 默认值确保相关结果不被过滤。

### 镜像选项（中国大陆）

```bash
mr-install --use-hf-mirror           # 使用 hf-mirror.com
mr-install --mirror-base-url <URL>   # 自定义镜像
```

### 跳过步骤

```bash
mr-install --skip-install   # 跳过 cargo install（已手动安装二进制）
mr-install --skip-model     # 跳过模型下载
mr-install --skip-service   # 跳过服务注册
mr-install --skip-verify    # 跳过安装验证
```

## 服务管理

### Linux (systemd)

```bash
systemctl --user status memrecd     # 查看状态
systemctl --user stop memrecd       # 停止
systemctl --user restart memrecd    # 重启
journalctl --user -u memrecd -f     # 查看日志
```

服务文件：`~/.config/systemd/user/memrecd.service`

### macOS (launchd)

```bash
launchctl list com.itcraft.memrecd               # 查看状态
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.itcraft.memrecd.plist   # 停止
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.itcraft.memrecd.plist # 启动
```

配置文件：`~/Library/LaunchAgents/com.itcraft.memrecd.plist`

特性：RunAtLoad + KeepAlive，登录即启动，崩溃自动重启。

### 重要：重启守护进程

重启 memrecd 时，需要先删除残留的 socket 文件：

```bash
rm -f ~/.memrec/memrecd.sock
systemctl --user restart memrecd   # 或直接：memrecd
```

使用 BGE-M3 时，模型加载 + 向量重建约需 75-90 秒，socket 才可用。

## 命令

### 命令一览

| 命令 | 用途 |
|------|------|
| `memrec add` | 添加记忆 |
| `memrec search` | 语义检索 |
| `memrec get` | 获取单条记忆 |
| `memrec list` | 列出记忆 |
| `memrec delete` | 删除记忆 |
| `memrec stats` | 统计信息 |
| `memrec version` | 版本信息 |

### 添加记忆

```bash
memrec add "内容" --mtype <类型> [--tag <标签>] [--global] [--source <来源>] [--scope <范围>]
```

#### 记忆类型

| 类型 | 标识 | 用途 | 推荐标签 |
|------|------|------|----------|
| 决策 | `decision` | 关键技术/业务决策 | `--tag critical` |
| 知识 | `knowledge` | 知识点、最佳实践、事实 | 见下方细分 |
| 上下文 | `context` | 项目配置、环境信息 | 项目相关标签 |
| 偏好 | `preference` | 用户偏好 | `--global` |
| 对话 | `conversation` | 对话记录（默认） | - |

#### knowledge 通过 tag 细分

| tag | 用途 | 示例 |
|-----|------|------|
| `fact` | 物理定律、数学公式、客观事实 | `--tag fact --tag physics` |
| `best-practice` | 最佳实践、设计模式 | `--tag best-practice` |
| `algorithm` | 算法、公式推导 | `--tag algorithm` |
| `tool` | 工具使用技巧 | `--tag tool --tag rust` |

#### 示例

```bash
# 决策
memrec add "选择JWT认证方案，理由：无状态、易扩展" --mtype decision --tag auth --tag critical

# 事实类知识
memrec add "光速c=3×10⁸m/s，真空环境中恒定" --mtype knowledge --tag fact --tag physics

# 最佳实践
memrec add "RAII模式：资源获取即初始化，析构自动释放" --mtype knowledge --tag best-practice --tag rust

# 项目上下文
memrec add "技术栈：Rust+Tokio+RocksDB，通信：Unix Socket" --mtype context --tag tech

# 用户偏好（公共记忆）
memrec add "用户偏好详细输出" --mtype preference --tag output --global
```

#### 公共记忆 vs 项目记忆

```bash
# 项目记忆（默认）：仅当前项目可检索
memrec add "项目A的数据库选型" --mtype decision --tag critical

# 公共记忆：所有项目可检索
memrec add "用户偏好暗色主题" --mtype preference --tag ui --global
```

#### 来源与范围

| 参数 | 值 | 说明 |
|------|------|------|
| `--source` | `user`（默认）、`system`、`inferred`、`external` | 记忆来源 — 影响搜索排序权重 |
| `--scope` | `project`（默认）、`global`、`workspace` | 记忆可见范围 |

```bash
# 用户来源记忆（搜索权重最高）
memrec add "我的偏好：使用tab而非空格" --mtype preference --source user --global

# 系统来源记忆
memrec add "自动检测：项目使用Rust 1.75" --mtype context --source system

# 推断知识
memrec add "根据代码模式推断偏好函数式风格" --mtype knowledge --source inferred
```

#### 长文本自动拆分

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

### 混合检索

MemRec 结合 KNN 向量搜索与 BM25 全文检索，提供最优搜索结果。

```bash
memrec search "查询" [选项]
```

#### 搜索流程

1. **KNN + BM25**：并行搜索，合并归一化分数
2. **时间衰减**：近期记忆权重更高（knowledge/decision 豁免）
3. **来源权重**：用户记忆权重高于系统/推断
4. **MMR 重排**：结果多样性，减少冗余

#### 中文搜索

通过 N-gram 分词器（2-4 字）支持中文全文检索，无需额外配置。

```bash
memrec search "中文搜索" --human
```

#### 搜索范围

| 选项 | 范围 | 用途 |
|------|------|------|
| 默认 | 当前项目 + 公共记忆 | 日常使用 |
| `--project-only` | 仅当前项目 | 精确项目内搜索 |
| `--global-only` | 仅公共记忆 | 查找用户偏好 |
| `--all` | 所有项目 | 跨项目关联搜索 |

#### 选项

| 选项 | 说明 | 默认值 |
|------|------|--------|
| `-k, --top-k` | 返回结果数 | 10 |
| `--min-score` | 最低相似度阈值 | 0.5（BGE-M3）/ 0.75（MiniLM） |
| `--project-only` | 仅当前项目 | - |
| `--global-only` | 仅公共记忆 | - |
| `--all` | 跨所有项目 | - |
| `--mtype` | 按类型过滤 | - |
| `--human` | 人类可读输出 | - |
| `--hybrid-alpha` | KNN与BM25权重（0=纯BM25，1=纯KNN） | 0.5 |
| `--mmr-enabled` | 启用MMR重排 | true |
| `--mmr-lambda` | MMR多样性（0=最大多样性，1=最大相关性） | 0.7 |

#### 示例

```bash
# 基本搜索
memrec search "认证方案"

# 项目内精确搜索
memrec search "性能优化" --project-only

# 公共记忆搜索
memrec search "用户偏好" --global-only

# 跨项目搜索
memrec search "xlsb" --all

# 调整返回数量和阈值
memrec search "架构" -k 20 --min-score 0.6

# 按类型过滤
memrec search "决策" --mtype decision

# 人类可读格式
memrec search "架构" --human
```

#### 相似度评分

**MiniLM-L6-v2：**

| score 范围 | 含义 |
|-----------|------|
| 0.9+ | 高度相关，精确匹配 |
| 0.8-0.9 | 相关，语义相近 |
| 0.75-0.8 | 基本相关 |
| < 0.75 | 可能不相关（被默认过滤） |

**BGE-M3：**

| score 范围 | 含义 |
|-----------|------|
| 0.7+ | 高度相关，精确匹配 |
| 0.5-0.7 | 相关，语义相近 |
| 0.4-0.5 | 基本相关 |
| < 0.4 | 可能不相关（被默认过滤） |

**调整阈值：**

```bash
# 临时调整
memrec search "query" --min-score 0.6

# 全局调整（环境变量）
export MEMREC_MIN_SCORE=0.6
memrec search "query"
```

#### 输出格式

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

### 获取记忆详情

```bash
# 获取单条记忆
memrec get <memory-id>

# 获取分块记忆的完整内容
memrec get <memory-id> --merge
```

### 列出记忆

```bash
memrec list                  # 默认列出20条
memrec list --limit 50       # 列出50条
memrec list --project-only   # 仅当前项目
memrec list --global-only    # 仅公共记忆
```

### 删除记忆

```bash
memrec delete <memory-id>
```

删除为软删除，记忆标记为 `is_deleted=true`。

### 统计信息

```bash
memrec stats
```

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
├── .mr_pid               # 项目ID文件（勿提交git）
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

### 隔离示例

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

## 配置

### config.toml

位置：`~/.memrec/config.toml`

**MiniLM-L6-v2 配置：**

```toml
version = "0.3.0"

[model]
model_type = "minilm-l6-v2"
source = "huggingface"
dimension = 384

[[model.files]]
filename = "model.onnx"
remote_path = "model.onnx"
sha256 = "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
file_type = "onnx-model"
required = true

[[model.files]]
filename = "tokenizer.json"
remote_path = "tokenizer.json"
sha256 = "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"
file_type = "tokenizer"
required = true

# ... 更多文件

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vectors_dir = "~/.memrec/vectors"
log_file = "~/.memrec/memrecd.log"
```

**BGE-M3 配置：**

```toml
version = "0.3.0"

[model]
model_type = "bge-m3"
source = "huggingface"
dimension = 1024

[[model.files]]
filename = "model.onnx"
remote_path = "onnx/model.onnx"
sha256 = "f84251230831afb359ab26d9fd37d5936d4d9bb5d1d5410e66442f630f24435b"
file_type = "onnx-model"
required = true

[[model.files]]
filename = "model.onnx_data"
remote_path = "onnx/model.onnx_data"
sha256 = "1eebfb28493f67bba03ce0ef64bfdc7fc5a3bd9d7493f818bb1d78cd798416b4"
file_type = "onnx-external-data"
required = true

# ... 更多文件

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vectors_dir = "~/.memrec/vectors"
log_file = "~/.memrec/memrecd.log"
```

### 环境变量

| 变量 | 用途 | 默认值 |
|------|------|--------|
| `MEMREC_MODEL_DIR` | 自定义模型路径 | `~/.memrec/models/<模型目录>/` |
| `MEMREC_MIN_SCORE` | 语义搜索最低相似度 | 0.75（MiniLM）/ 0.5（BGE-M3） |
| `RUST_LOG` | 日志级别 | `info` |

## 数据目录

```
~/.memrec/
├── config.toml           # 配置文件
├── memrecd.sock          # Unix Socket（运行时生成）
├── memrecd.log           # 服务日志
├── data/                 # RocksDB 记忆元数据
├── vectors/              # RocksDB 向量存储
├── models/               # ONNX Embedding 模型
│   ├── Qdrant--all-MiniLM-L6-v2-onnx/   # MiniLM-L6-v2
│   │   ├── model.onnx
│   │   ├── tokenizer.json
│   │   ├── config.json
│   │   ├── special_tokens_map.json
│   │   └── tokenizer_config.json
│   └── BAAI--bge-m3/                     # BGE-M3
│       ├── model.onnx
│       ├── model.onnx_data
│       ├── Constant_7_attr__value
│       ├── tokenizer.json
│       ├── config.json
│       ├── special_tokens_map.json
│       ├── tokenizer_config.json
│       └── sentencepiece.bpe.model
└── logs/                 # 日志目录
```

## 切换模型

从 MiniLM 切换到 BGE-M3（或反之）：

1. **使用新模型重新安装：**
   ```bash
   mr-install --model bge-m3 --skip-install --skip-service
   ```

2. **备份现有向量：**
   ```bash
   mv ~/.memrec/vectors ~/.memrec/vectors.minilm.bak
   ```

3. **重启守护进程**（向量将自动重建）：
   ```bash
   rm -f ~/.memrec/memrecd.sock
   systemctl --user restart memrecd
   ```

4. **等待重建完成**（BGE-M3 约 500 条记忆需 ~75-90 秒）

**重要：** 切换模型需要重建所有向量，因为不同模型产生不同维度的 embedding。旧向量与新模型不兼容。

## MCP Server

MemRec 支持 Model Context Protocol (MCP)，可直接与 AI 客户端集成：

```bash
memrec --mcp    # 以 stdio 模式启动 MCP 服务器
```

### MCP 工具

| 工具 | 用途 |
|------|------|
| `mr_add` | 添加记忆 |
| `mr_search` | 语义检索 |
| `mr_get` | 获取记忆详情 |
| `mr_list` | 列出记忆 |
| `mr_delete` | 删除记忆 |
| `mr_stats` | 获取统计信息 |

### MCP 资源

| 资源 | 用途 |
|------|------|
| `memrec://stats` | 系统统计 |
| `memrec://project` | 当前项目信息 |

## AI Agent 工作流

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

| 指标 | MiniLM-L6-v2 | BGE-M3 |
|------|-------------|--------|
| 启动时间 | < 50ms | ~75-90秒（模型加载+向量重建） |
| 请求延迟 | < 1ms | < 1ms |
| 搜索延迟 | < 5ms | < 10ms |
| 内存占用 | ~118MB | ~1.5GB |
| 向量维度 | 384 | 1024 |
| 模型磁盘大小 | ~90MB | ~2.3GB |

## 常见问题

### 启动报错 "Failed to connect to memrecd"

守护进程未运行。启动方法：
```bash
memrecd
# 或
systemctl --user start memrecd
```

### 语义搜索返回0条结果

1. 检查模型文件：`ls ~/.memrec/models/`
2. 检查服务日志：`cat ~/.memrec/memrecd.log`
3. 降低 min_score：`memrec search "query" --min-score 0.3`
4. BGE-M3 默认 min_score 为 0.5 — 尝试降低到 0.3-0.4

### 项目记忆没有隔离

1. 确认 `.mr_pid` 文件存在于项目根目录
2. 确认在项目目录内执行命令（不是 `~` 或 `/tmp`）
3. git 仓库会自动检测 git root

### 模型下载失败（中国大陆网络）

```bash
mr-install --use-hf-mirror
```

### 切换模型

1. 使用新模型重新安装：`mr-install --model <模型> --skip-install --skip-service`
2. 备份向量：`mv ~/.memrec/vectors ~/.memrec/vectors.bak`
3. 重启守护进程：`rm -f ~/.memrec/memrecd.sock && systemctl --user restart memrecd`

## 升级

```bash
mr-install
```

重新运行 mr-install 即可升级（cargo install 会覆盖旧版本，服务会重启）。

## 卸载

```bash
# Linux
systemctl --user stop memrecd
systemctl --user disable memrecd
rm ~/.config/systemd/user/memrecd.service
systemctl --user daemon-reload
rm ~/.local/bin/memrec ~/.local/bin/memrecd ~/.local/bin/mr-install

# macOS
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.itcraft.memrecd.plist
rm ~/Library/LaunchAgents/com.itcraft.memrecd.plist
rm ~/bin/memrec ~/bin/memrecd ~/bin/mr-install

# 删除数据（可选，会清除所有记忆）
rm -rf ~/.memrec
```
