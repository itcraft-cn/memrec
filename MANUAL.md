# MemRec 使用手册

> [English](MANUAL_en.md) | 中文

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
launchctl list cn.itcraft.memrecd               # 查看状态
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/cn.itcraft.memrecd.plist   # 停止
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/cn.itcraft.memrecd.plist # 启动
```

配置文件：`~/Library/LaunchAgents/cn.itcraft.memrecd.plist`

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
| `memrec restore` | 恢复软删除记忆（重新加入检索索引） |
| `memrec stats` | 统计信息 |
| `memrec scene` | L2 场景聚合（create/list/get/add-memory/remove-memory/update-heat/delete） |
| `memrec dream` | 触发 Dream 整合（`--force`、`--dry-run`） |
| `memrec dream-report` | 查看最近若干次 DreamRun 审计报告 |
| `memrec eval` | 离线检索评测（JSONL 数据集，输出命中率指标） |
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
| 默认 | 按 `.mr_pid` 推断 | 已注册项目→仅当前项目；未注册→仅公共记忆 |
| `-p, --project` | 仅当前项目 | 精确项目内搜索 |
| `-g, --global` | 仅公共记忆 | 查找用户偏好 |
| `-a, --all` | 所有项目 | 跨项目关联搜索 |

> 三个范围参数互斥。均未指定时，检测当前项目根目录是否存在 `.mr_pid` 标记文件：
> 存在则默认 `-p`（仅当前项目），不存在则默认 `-g`（仅公共记忆）。

#### 选项

| 选项 | 说明 | 默认值 |
|------|------|--------|
| `-k, --top-k` | 返回结果数 | 10 |
| `--min-score` | 最低相似度阈值 | 0.5（BGE-M3）/ 0.75（MiniLM） |
| `-p, --project` | 仅当前项目 | 按 `.mr_pid` 推断 |
| `-g, --global` | 仅公共记忆 | 按 `.mr_pid` 推断 |
| `-a, --all` | 跨所有项目 | - |
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
memrec search "性能优化" -p

# 公共记忆搜索
memrec search "用户偏好" -g

# 跨项目搜索
memrec search "xlsb" -a

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
memrec list -p               # 仅当前项目（有 .mr_pid 时默认）
memrec list -g               # 仅公共记忆（无 .mr_pid 时默认）
memrec list -a               # 跨所有项目
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

### Dream 整合与审计

Dream 在系统空闲时自动触发，也可手动执行；整合采用两段式管线（Stage F 融合 + Stage N 成网），详见 README「Dream 记忆整合」。每次运行（含门控拦截、dry-run、失败）都会落库一条 `DreamRun` 审计记录。

```bash
memrec dream --force              # 手动触发整合（忽略时间/新增门槛）
memrec dream --dry-run            # 预演：只计算不写库、不删数据、不建边
memrec dream-report --last 5      # 查看最近 5 次 DreamRun（候选/簇/边/传播/状态）
```

### 离线检索评测

以 JSONL 数据集驱动只读检索，输出 `hit@k`/`recall@k`/`MRR`/`nDCG`，用于对比不同检索配置（如开启/关闭图扩散）。

```bash
memrec eval dataset.jsonl --k 10 --human
```

数据集每行格式：`{"query": "...", "relevant": ["<uuid>", ...]}`。

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
memrec search "架构" -p          # 仅返回 memrec 项目

# 跨项目搜索
memrec search "架构" -a          # 返回所有项目的架构相关记忆
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
log_file = "~/.memrec/logs/memrecd.log"
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
log_file = "~/.memrec/logs/memrecd.log"
```

### 环境变量

| 变量 | 用途 | 默认值 |
|------|------|--------|
| `MEMREC_MODEL_DIR` | 自定义模型路径 | `~/.memrec/models/<模型目录>/` |
| `MEMREC_MIN_SCORE` | 语义搜索最低相似度 | 0.75（MiniLM）/ 0.5（BGE-M3） |
| `MEMREC_LLM_KEY` | LLM API Key（`[llm] api_key` 可引用 `"$MEMREC_LLM_KEY"`；systemd 服务经 `~/.memrec/memrecd.env` 加载） | 空 |
| `RUST_LOG` | 日志级别 | `info` |

### LLM 与 Dream 配置

可选接入 OpenAI 兼容大模型驱动 Dream 整合摘要；`[dream]` 控制整合行为。

```toml
[llm]
enabled = false            # 设为 true 解锁 Dream 摘要
provider = "deepseek"       # deepseek / openai / custom（任意 OpenAI 兼容服务）
url = "https://api.deepseek.com/v1"
api_key = ""                # 支持环境变量引用（如 "$MEMREC_LLM_KEY"）
model = "deepseek-chat"
think_level = "medium"      # off / minimal / low / medium / high / xhigh
timeout_secs = 120
max_tokens = 4096
temperature = 0.3

[dream]
enabled = false
requires_llm = true
min_memories = 20           # 最少可整合记忆数
max_age_hours = 168         # 整合的记忆最大年龄（小时）
min_hours_between = 24.0    # 两次整合最小间隔（小时）
min_new_memories = 1        # 距上次执行后用户新增记忆数门槛
integration_max_memories = 100        # 单次整合输入上限（重要性升序选取，防 token 超窗）
integration_preserve_importance = 0.85  # 高重要性记忆保留（不整合）
integration_preserve_tags = ["critical"]  # 命中标签保留
integration_preserve_types = ["preference", "context"]  # 保留的记忆类型（原文，context 压缩后细节不可恢复）
integration_min_content_len = 10      # 过滤过短测试/占位记录（字符）

# 检索图扩散（1 跳；图命中带 EdgeHint）
retrieval_graph_enabled = true   # 检索是否走图扩散
retrieval_graph_depth = 1        # 扩散跳数
retrieval_graph_decay = 0.6      # 扩散衰减
retrieval_graph_weight = 0.3     # 图命中并入候选的权重
retrieval_graph_quota = 0.25     # 图命中在 top_k 中的保留名额比例（0=退回纯按分截断）

# 成网（Stage N）
networking_enabled = true               # 是否建边成网
networking_similar_topk = 5             # Similar 边近邻数
networking_similar_threshold = 0.82     # Similar 边阈值
networking_semantic_enabled = false     # 是否启用 LLM 语义边（默认关）
networking_semantic_max_candidates = 20 # 语义边候选池按向量相似度限幅
networking_semantic_dup_threshold = 0.97 # 语义边候选近重复排除阈值
networking_propagation_enabled = true   # 是否启用边传播

# 上游去重（Stage F 事实条近重复）
fusion_dedup_enabled = true     # 对融合产出事实条做近重复去重
fusion_dedup_threshold = 0.97   # 近重复判定阈值（余弦）

[dedup]                     # 写入去重（ADD/UPDATE/NOOP）
enabled = true
top_k = 5
duplicate_threshold = 0.92  # 高于此值视为重复，跳过
update_threshold = 0.85     # 高于此值合并更新已有记忆
update_window_mins = 60     # 原地更新的时间窗口（分钟）；窗口外新增并标记旧记录 superseded；0 = 不限制

[log]
enabled = true             # 文件日志（false 时仅输出 stdout）
level = "info"             # trace/debug/info/warn/error
rotation_days = 7          # 按天滚存保留最近 N 个文件（本地时区 0 点切换）
memrecd_log = "~/.memrec/logs/memrecd.log"  # 日志完整路径（本地时区时间戳，含 +08:00 偏移）
```

> systemd 用户服务不继承 shell 环境变量：将 API 密钥写入 `~/.memrec/memrecd.env`（权限 600，服务模板已通过 `EnvironmentFile` 加载）后 `systemctl --user restart memrecd`；终端手动启动时直接 `export` 即可。

## 数据目录

```
~/.memrec/
├── config.toml           # 配置文件
├── memrecd.sock          # Unix Socket（运行时生成）
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
└── logs/                 # 日志目录（memrecd.YYYY-MM-DD.log 按天滚存，默认保留 7 个）
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
| `mr_dream` | 触发 Dream 整合（`force`、`dry_run`） |
| `mr_dream_report` | 查看最近若干次 DreamRun 审计报告 |

### MCP 资源

| 资源 | 用途 |
|------|------|
| `memrec://stats` | 系统统计 |
| `memrec://project` | 当前项目信息 |

## AI Agent 工作流

```bash
# 1. 开始任务：检索相关历史
memrec search "相关主题" -p

# 2. 做出决策后：记录
memrec add "选择XXX方案，理由：..." --mtype decision --tag critical

# 3. 用户表达偏好：记录为公共记忆
memrec add "用户偏好YYY" --mtype preference --global

# 4. 发现跨项目关联
memrec search "相关主题" -a

# 5. 结束任务
memrec stats
```

### Skill 集成

AI CLI 工具（如 opencode）可通过 Skill 集成：
- Skill 文件：`~/.opencode/skills/memrec/SKILL.md`
- AI agent 自动读取 Skill，了解命令用法和最佳实践

## DeepSeek Harness (DSH) 插件集成

MemRec 提供 DSH 原生插件（仓库 `dsh-plugin/` 目录，npm 包 `@memrec/dsh-plugin`），
比 MCP 桥接更深度：

| 能力 | 说明 |
|------|------|
| 原生工具 | `mr_add` / `mr_search` / `mr_get` / `mr_list` / `mr_delete` / `mr_stats` / `mr_dream`，无 `mcp__` 前缀 |
| 项目隔离 | 工具自动携带会话工作目录（MCP 模式不传 `working_dir`，项目隔离失效；插件修复此问题） |
| 上下文注入 | 模型请求前自动检索当前项目相关记忆注入会话 |
| 自动记录 | turn 结束时 LLM 提取决策/知识/偏好写入 memrec（`source: inferred`） |

启用步骤：

```bash
# 1. 构建插件
cd dsh-plugin && npm install && npm run build

# 2. 安装到 profile（如 web）
dsh plugin --profile web add /abs/path/to/memrec/dsh-plugin

# 3. 在 $DSH_HOME/profiles/web/cordis.patch.yml 追加：
# - insert:
#     - id: memrec
#       name: '/abs/path/to/memrec/dsh-plugin/lib/index.js'
#       config:
#         socketPath: '~/.memrec/memrecd.sock'
#         injectContext: true
#         autoRecord: true

# 4. 重启 DSH 进程后生效
```

配置项（均可选）：`socketPath`、`injectContext`、`contextTopK`、
`contextMinScore`、`contextRefreshTurns`、`autoRecord`、
`autoRecordProvider`、`autoRecordModel`、`autoRecordInterval`、
`autoRecordMaxPerSession`。详见 `dsh-plugin/README.md` 与
`docs/design/dsh-plugin-design.md`。

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
2. 检查服务日志：`cat ~/.memrec/logs/memrecd.log`
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
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/cn.itcraft.memrecd.plist
rm ~/Library/LaunchAgents/cn.itcraft.memrecd.plist
rm ~/bin/memrec ~/bin/memrecd ~/bin/mr-install

# 删除数据（可选，会清除所有记忆）
rm -rf ~/.memrec
```
