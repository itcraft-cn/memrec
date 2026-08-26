[English](README.md) | 中文

# MemRec — AI记忆持久化系统

> 本地优先的AI记忆，项目隔离 — 为终端而生，为私密而用

本地化记忆持久化系统，为AI CLI工具提供跨会话记忆恢复、知识积累、对话存档能力。

## 特性

- **项目隔离** — 自动检测git root，.mr_pid持久化，多项目独立记忆
- **混合检索** — KNN向量搜索 + BM25全文检索，MMR重排，中文搜索支持
- **场景聚合** — L2场景聚合层，按主题聚合记忆卡片，热度排序（记忆金字塔 L0-L3）
- **Dream 整合** — 空闲时自动触发的四阶段记忆整合，可选 LLM 摘要
- **LLM 接入** — 可选 OpenAI 兼容大模型，驱动 Dream 摘要
- **写入去重** — 每次写入 ADD/UPDATE/NOOP 决策
- **插件生态** — pi 扩展与 DeepSeek Harness 原生插件
- **语义检索** — 本地ONNX模型 — MiniLM-L6-v2（384维）或 BGE-M3（1024维，多语言），零API费用
- **智能评分** — 时间衰减、常青豁免、来源权重
- **跨项目搜索** — `--all`标志发现跨项目关联知识
- **AI-first设计** — 默认JSON输出，命令简洁，Skill集成
- **高性能** — Rust实现，<1ms延迟，~118MB（MiniLM）/ ~1.5GB（BGE-M3）内存（含模型）
- **长文本拆分** — >7.5KB自动拆分为chunks
- **守护进程管理** — Linux systemd / macOS launchd / Windows Startup

## 快速开始

### 安装

```bash
# 一条命令完成全部安装
cargo install --locked mr-install
mr-install
```

`mr-install` 自动完成：
1. 通过 `cargo install` 安装 memrec/memrecd
2. 创建 `~/.memrec/` 目录结构
3. 下载 ONNX Embedding 模型（~90MB）
4. 注册并启动守护进程服务
5. 验证安装

| 平台 | 二进制路径 | 数据路径 |
|------|-----------|---------|
| Linux | `~/.local/bin/` | `~/.memrec/` |
| macOS | `~/bin/` | `~/.memrec/` |

### 选择 Embedding 模型

| 模型 | 维度 | 适用场景 | 磁盘空间 | 内存 |
|------|------|----------|----------|------|
| `minilm-l6-v2`（默认） | 384 | 纯英文 | ~90MB | ~118MB |
| `bge-m3` | 1024 | 中文/多语言 | ~2.3GB | ~1.5GB |

```bash
# 默认：MiniLM-L6-v2（英文）
mr-install

# BGE-M3（中文/多语言，中文用户推荐）
mr-install --model bge-m3
```

模型下载镜像选项：

```bash
mr-install --use-hf-mirror           # 使用 hf-mirror.com（国内）
mr-install --mirror-base-url <URL>   # 自定义镜像
```

### 使用

```bash
# 添加记忆
memrec add "选择JWT认证方案" --mtype decision --tag critical
memrec add "RAII模式：资源获取即初始化" --mtype knowledge --tag best-practice --tag rust
memrec add "用户偏好详细输出" --mtype preference --tag output --global

# 混合检索（KNN + BM25）
memrec search "认证方案"                   # 当前项目 + 公共，min_score 默认值：0.5（BGE-M3）/ 0.75（MiniLM）
memrec search "性能优化" --project-only    # 仅当前项目
memrec search "用户偏好" --global-only     # 仅公共记忆
memrec search "xlsb" --all                 # 跨所有项目
memrec search "中文搜索" --human           # 中文搜索支持

# 其他
memrec list --limit 20
memrec list --limit 10 --skip 20        # 分页：跳过前 20 条
memrec list --tag rust --type decision   # 按标签/类型过滤
memrec list --deleted                    # 从 RocksDB 检索软删除的原始记录
memrec get <id>
memrec restore <id>                      # 恢复软删除记忆（重新入检索索引）
memrec stats
memrec version

# L2 场景聚合
memrec scene create "Rust开发规范" --tag rust
memrec scene list --sort-by-heat
memrec scene add-memory <scene-id> <memory-id>

# Dream 整合（空闲自动触发，或手动）
memrec dream --force
```

## 项目隔离

MemRec 自动为不同项目创建独立的记忆空间：

```
project-a/           project-b/
├── .mr_pid          ├── .mr_pid        ← 自动创建，不同ID
├── .gitignore       ├── .gitignore     ← 建议添加 .mr_pid
└── src/             └── src/
```

- **git仓库**：自动检测 git root
- **非git目录**：使用当前工作目录
- **公共记忆**：`--global` 标记，所有项目可检索
- **跨项目搜索**：`--all` 搜索所有项目

## 记忆类型

| 类型 | 标识 | 用途 |
|------|------|------|
| 决策 | `decision` | 关键技术/业务决策 |
| 知识 | `knowledge` | 知识点（通过tag细分：`fact`/`best-practice`/`algorithm`/`tool`） |
| 上下文 | `context` | 项目配置、环境信息 |
| 偏好 | `preference` | 用户偏好（推荐 `--global`） |
| 对话 | `conversation` | 对话记录（默认） |

## 场景聚合（L2）

记忆按金字塔组织：L0 流水账 → L1 记忆卡片 → L2 场景档案 → L3 人格摘要（暂不实现）。L2 场景层按主题聚合 L1 记忆卡片，并按访问热度排序：

```bash
memrec scene create "Rust开发规范" --tag rust   # 创建场景
memrec scene list --sort-by-heat                 # 按热度排序列表
memrec scene add-memory <scene-id> <memory-id>   # 关联记忆卡片
memrec scene remove-memory <scene-id> <memory-id>
memrec scene get <scene-id>                      # 场景详情（含证据链）
memrec scene update-heat <scene-id> 0.8
```

每个场景包含 `theme`、`memory_ids`、`heat`、`tags`；热度随记忆访问上升，热点主题优先可见。设计文档：[L2-scene-aggregation-design.md](docs/design/L2-scene-aggregation-design.md)

## LLM 接入（OpenAI 兼容）

可选接入 OpenAI 兼容大模型，用于 **Dream 整合摘要**。
未启用（`[llm].enabled = false`）时，Dream 视为不启用。

```toml
# ~/.memrec/config.toml
[llm]
enabled = false            # 设为 true 解锁 Dream 摘要
provider = "deepseek"       # deepseek / openai / custom（任意 OpenAI 兼容服务）
url = "https://api.deepseek.com/v1"
api_key = ""                # API Key
model = "deepseek-chat"     # 如 deepseek-reasoner、gpt-4o-mini
think_level = "medium"      # off / minimal / low / medium / high / xhigh
timeout_secs = 120
max_tokens = 4096
temperature = 0.3
```

`think_level` 按 provider 映射：

| think_level | DeepSeek（`thinking` 参数） | OpenAI（`reasoning_effort`） |
|-------------|------------------------------|------------------------------|
| `off`       | 不发送                       | 不发送                       |
| `minimal`/`low`/`medium`/`high` | `{type: enabled, effort: high}` | `minimal`/`low`/`medium`/`high` |
| `xhigh`     | `{type: enabled, effort: max}` | `high`                        |

## 写入去重（ADD/UPDATE/NOOP）

每次 `add` 时检索相似记忆（余弦相似度）并决策：

- `score >= duplicate_threshold` → **跳过**（NOOP），不写入
- `update_threshold <= score < duplicate_threshold` → **更新**已有记忆（合并 tags、内容取新）
- 否则 → **新增**

```toml
[dedup]
enabled = true
top_k = 5
duplicate_threshold = 0.92   # 高于此值视为重复，跳过
update_threshold = 0.85      # 高于此值合并更新已有记忆
```

单次调用可用 `--no-dedup`（CLI）或 `"dedup": false`（协议）关闭。
决策层可插拔：当前为规则版，LLM 版（真正的 DELETE/矛盾消解）预留演进。

## Dream 记忆整合

Dream 是后台记忆整合作业，类比睡眠中的记忆巩固——系统空闲时自动触发，或通过 `memrec dream --force` 手动触发。共 4 个阶段：

1. **CrossProjectExtract** — 提取跨项目共性主题
2. **PersonalSummary** — 聚合全局记忆，生成个人画像
3. **Cleanup** — 清理过期、低重要性记忆
4. **VectorRegen** — 为缺失向量重新生成嵌入（资源消耗大，默认关闭）

门槛条件：至少 `min_memories`（20）条记忆、最老记忆年龄 ≥ `max_age_hours`（168h）、距上次执行 ≥ `min_hours_between`（24h）。配置 `[llm]` 后，整合会生成 LLM 摘要记忆，标记 `dream-integrated`。

```toml
[dream]
enabled = false            # 设为 true 解锁后台整合
requires_llm = true        # 未配置 [llm] 时门槛检查失败
min_memories = 20
max_age_hours = 168
min_hours_between = 24.0
phase_vector_regen = false # 阶段4，资源消耗大，默认关闭

[idle]
enabled = true             # 空闲监控
check_interval_secs = 300  # 每 5 分钟检查一次
load_threshold = 0.5       # load average 低于此值视为空闲
auto_dream_enabled = true  # 空闲时自动触发 Dream

[queue]
normal_queue_capacity = 1000
dream_queue_capacity = 10

[log]
enabled = true
rotation_days = 7          # 日志保留 7 天
```

## 插件

| 插件 | 平台 | 能力 |
|------|------|------|
| `pi-extensions/memrec.ts` | pi 编码代理 | 6 个 LLM 工具（`memrec_search`/`add`/`get`/`list`/`delete`/`stats`）+ agent 启动自动注入上下文；命令 `/memrec`、`/memrec-auto` |
| `dsh-plugin/` | DeepSeek Harness | 7 个原生工具（`mr_add`/`mr_search`/`mr_get`/`mr_list`/`mr_delete`/`mr_stats`/`mr_dream`）+ 上下文注入 + 会话自动记录（LLM 提取） |

- **pi**：将 `pi-extensions/memrec.ts` 复制到 `~/.pi/agent/extensions/`，然后 `/reload`
- **DSH**：详见 [dsh-plugin/README.md](dsh-plugin/README.md)

## 数据位置

```
~/.memrec/
├── config.toml           # 配置文件
├── memrecd.sock          # Unix Socket
├── data/                 # RocksDB 记忆元数据
├── vectors/              # RocksDB 向量存储
└── models/               # ONNX Embedding 模型
    ├── Qdrant--all-MiniLM-L6-v2-onnx/   # MiniLM-L6-v2（默认）
    │   ├── model.onnx
    │   ├── tokenizer.json
    │   └── ...
    └── BAAI--bge-m3/                     # BGE-M3（多语言）
        ├── model.onnx
        ├── model.onnx_data
        ├── tokenizer.json
        └── ...
```

## 环境变量

| 变量 | 用途 | 默认值 |
|------|------|--------|
| `MEMREC_MODEL_DIR` | 自定义模型路径 | `~/.memrec/models/<模型目录>/`（模型相关） |
| `MEMREC_MIN_SCORE` | 语义搜索最低相似度 | `0.75（MiniLM）/ 0.5（BGE-M3）` |
| `RUST_LOG` | 日志级别 | `info` |

## 文档

- [使用手册](MANUAL_cn.md)
- [Manual (English)](MANUAL.md)
- [Skill文档](skills/memrec/SKILL.md)
- [L2 场景聚合设计](docs/design/L2-scene-aggregation-design.md)
- [DSH 插件设计](docs/design/dsh-plugin-design.md)
- [空闲触发 Dream 设计](docs/superpowers/specs/2026-07-30-idle-dream-design.md)

## 项目结构

```
memrec/
├── common/       # 共享类型和协议
├── memrecd/      # 守护进程服务
├── memrec/       # CLI工具
├── mr-install/   # 安装器
├── mr-ability/   # 核心能力库（检索/嵌入/Dream/LLM/去重/队列/空闲监控）
├── mr-common/    # 共享类型
├── mr-protocol/  # JSON-RPC 协议
├── dsh-plugin/   # DeepSeek Harness 插件
├── pi-extensions/# pi 扩展
└── docs/         # 文档
```

## 许可证

Apache-2.0

## 更新日志

详见 [CHANGELOG_cn.md](CHANGELOG_cn.md)
