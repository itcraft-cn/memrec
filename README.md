[English](README_en.md) | 中文

# MemRec — AI记忆持久化系统

> 本地优先的AI记忆，项目隔离 — 为终端而生，为私密而用

本地化记忆持久化系统，为AI CLI工具提供跨会话记忆恢复、知识积累、对话存档能力。

## 特性

- **项目隔离** — 自动检测git root，.mr_pid持久化，多项目独立记忆
- **混合检索** — KNN向量搜索 + BM25全文检索，MMR重排（相似度以全文计算），二级索引下推查询，中文搜索支持
- **场景聚合** — L2场景聚合层，按主题聚合记忆卡片，热度排序（记忆金字塔 L0-L3）
- **Dream 整合** — 空闲时自动触发的四阶段记忆整合，可选 LLM 摘要
- **Dream 两段式重构与审计** — Stage F 分簇提炼 + Stage N 成网；每次运行落库 `DreamRun`，`memrec dream-report` 可查，`--dry-run` 预演
- **检索图扩散** — 混合检索后按 1 跳图扩散补充召回，图命中带 EdgeHint；`retrieval_graph_quota` 为图命中预留 top_k 名额
- **离线检索评测** — `memrec eval` 以 JSONL 数据集输出 hit@k/recall@k/MRR/nDCG
- **LLM 接入** — 可选 OpenAI 兼容大模型，驱动 Dream 摘要（可选 LLM 语义边）
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
memrec search "认证方案"                   # 默认范围：有 .mr_pid → 仅当前项目；无 → 仅公共记忆
memrec search "性能优化" -p              # 仅当前项目
memrec search "用户偏好" -g              # 仅公共记忆
memrec search "xlsb" -a                  # 跨所有项目
memrec search "中文搜索" --human         # 中文搜索支持

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
memrec dream --dry-run                    # 预演：不写库/不删数据/不建边
memrec dream-report --last 5              # 查看最近若干次 DreamRun 审计
memrec eval dataset.jsonl --k 10          # 离线检索评测（JSONL）
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

提示: 若 daemon 由 **systemd 用户服务**启动（不继承 shell 环境变量），请将 API 密钥写入 `~/.memrec/memrecd.env`（权限 600，服务单元通过 `EnvironmentFile` 自动加载；`mr-install` 注册的服务模板已包含该配置）：

```bash
echo "MEMREC_LLM_KEY=sk-xxx" > ~/.memrec/memrecd.env && chmod 600 ~/.memrec/memrecd.env
systemctl --user restart memrecd
```

若在终端手动启动 `memrecd`，直接使用 shell 环境变量即可（`export MEMREC_LLM_KEY=sk-xxx`）。
不要将密钥写入 `~/.config/environment.d/`（该目录为系统共享配置，且修改后需重启 user manager 才生效）。

```toml
# ~/.memrec/config.toml
[llm]
enabled = false            # 设为 true 解锁 Dream 摘要
provider = "deepseek"       # deepseek / openai / custom（任意 OpenAI 兼容服务）
url = "https://api.deepseek.com/v1"
api_key = ""                # API Key，支持环境变量引用（如 "$MEMREC_LLM_KEY"）
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

`update_window_mins` 限定"更新"的时间窗口：窗口内命中则原地更新；窗口外命中不修改历史记录，而是新增一条并将旧记录标记为 `superseded`（合并与软删仅由 Dream 执行）。

```toml
[dedup]
enabled = true
top_k = 5
duplicate_threshold = 0.92   # 高于此值视为重复，跳过
update_threshold = 0.85      # 高于此值合并更新已有记忆
update_window_mins = 60      # 原地更新的时间窗口（分钟）；0 = 不限制
```

单次调用可用 `--no-dedup`（CLI）或 `"dedup": false`（协议）关闭。
决策层可插拔：当前为规则版，LLM 版（真正的 DELETE/矛盾消解）预留演进。

## Dream 记忆整合

Dream 是后台记忆整合作业，类比睡眠中的记忆巩固——系统空闲时自动触发，或通过 `memrec dream --force` 手动触发。

整合采用**两段式管线**：

1. **Stage F（融合）** — 零 LLM 语义聚类，按簇结构化提炼 `facts`/`summary`/`contradictions` 并记录 provenance（`metadata.source_ids`，用于幂等与簇域评级）
2. **Stage N（成网）** — 建立 `Similar`/`SameProject`/`References`/`Contradicts` 边供检索图扩散使用；`networking_semantic_enabled` 开启后追加 LLM 语义边（`CausedBy`/`Evolves`/`References`，默认关）

之后执行阶段 1-4：`CrossProjectExtract`（跨项目共性主题）、`PersonalSummary`（个人画像）、`Cleanup`（过期/低重要性清理）、`VectorRegen`（缺失向量重生成，默认关闭）。

每次运行（含门控拦截、dry-run、失败）落库一条 `DreamRun` 审计记录，可经 `memrec dream-report` 查看；`memrec dream --dry-run` 可预演（不写库、不删数据、不建边）。

门槛条件：至少 `min_memories`（20）条可整合记忆、最老记忆年龄 ≥ `max_age_hours`（168h）、距上次执行 ≥ `min_hours_between`（24h）、且距上次执行后**用户新增**记忆 ≥ `min_new_memories`（1，新记忆门槛，按 `created_at > last_run` 且非 System 来源统计）。配置 `[llm]` 后，整合会生成 LLM 摘要记忆，标记 `dream-integrated`。

**整合保留策略**（防止有价值记忆被压缩丢失）：

- `integration_preserve_importance`（0.85）— importance 达标记忆不整合
- `integration_preserve_tags`（`critical`）— 命中标签不整合
- `integration_preserve_types`（`preference` + `context`）— 用户偏好与配置/环境信息保留原文（context 压缩后细节不可恢复）
- `integration_min_content_len`（10 字符）— 过滤过短测试/占位垃圾
- `integration_max_memories`（100）— 单次整合输入上限，按重要性升序选取，防 LLM token 超窗

**健壮性**：仅执行成功才更新 `dream.state`（LLM 故障恢复后可立即重试）；整合记忆记录 `metadata.source_ids` 实现幂等（崩溃残留自动补删）；LLM 整合成功后才执行阶段 1-4；整合摘要由强约束模板产出（`## 主题`/`## 要点`/`## 结论`，无套话）。

```toml
[dream]
enabled = false            # 设为 true 解锁后台整合
requires_llm = true        # 未配置 [llm] 时门槛检查失败
min_memories = 20          # 最少可整合记忆数
max_age_hours = 168        # 整合的记忆最大年龄（小时）
min_hours_between = 24.0   # 两次整合最小间隔（小时）
min_new_memories = 1       # 距上次执行后用户新增记忆数门槛
integration_max_memories = 100        # 单次整合输入上限（重要性升序选取）
integration_preserve_importance = 0.85  # 高重要性记忆保留
integration_preserve_tags = ["critical"]  # 命中标签保留
integration_preserve_types = ["preference", "context"]  # 保留的记忆类型（原文）
integration_min_content_len = 10      # 过短内容过滤（字符）
cleanup_inactive_days = 90  # 阶段3：不活跃天数门槛
cleanup_importance_threshold = 0.1    # 阶段3：清理重要性下限
phase_vector_regen = false # 阶段4，资源消耗大，默认关闭

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

[idle]
enabled = true             # 空闲监控
check_interval_secs = 300  # 每 5 分钟检查一次
load_threshold = 0.5       # load average 低于此值视为空闲
auto_dream_enabled = true  # 空闲时自动触发 Dream

[queue]
normal_queue_capacity = 1000
dream_queue_capacity = 10

[log]
enabled = true             # 启用文件日志（false 时仅输出 stdout）
level = "info"             # 日志级别：trace/debug/info/warn/error
rotation_days = 7          # 按天滚存，保留最近 7 个日志文件（本地时区 0 点切换）
memrecd_log = "~/.memrec/logs/memrecd.log"  # 日志文件完整路径（本地时区时间戳）
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
| `MEMREC_LLM_KEY` | LLM API Key（`[llm] api_key` 可引用 `"$MEMREC_LLM_KEY"`；systemd 服务经 `~/.memrec/memrecd.env` 加载） | 空 |
| `RUST_LOG` | 日志级别 | `info` |

## 文档

- [使用手册](MANUAL.md)
- [Manual (English)](MANUAL_en.md)
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

详见 [CHANGELOG.md](CHANGELOG.md)（英文版 [CHANGELOG_en.md](CHANGELOG_en.md)）
