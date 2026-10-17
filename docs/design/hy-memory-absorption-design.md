# Hy Memory 可吸纳能力设计文档

> 状态：草稿
> 日期：2026-10-17
> 来源：腾讯混元 Hy Memory 2.0 公开资料（README / 论文 arXiv:2606.09483、arXiv:2610.22196、CMB 基准）
> 定位：对照 memrec 现状，识别可吸纳的记忆形态与工程机制，给出上、中、下三策与实施路径
> 说明：本文只做设计，不落地实现；文中少量伪代码仅用于讲清逻辑，不作为主要篇幅

---

## 1. 背景与目的

memrec 与 Hy Memory 属于同一赛道：本地优先、跨会话、面向 LLM Agent 的持久化记忆系统。但两者侧重不同：

- memrec 在**检索管线与生命周期**上更成熟：混合检索（KNN + BM25）、RRF 融合、MMR 多样性重排、时间衰减、源权重、图边扩散、规则引擎、分层呈现（token 预算）、Dream 后台整合、L0 SQLite 存档。
- Hy Memory 在**记忆形态的认知分层与任务域专化**上更完整：Chat 的 L0–L7 八层结构，Working 的"事实 + 可复用经验"双层，Coding 的版本演进链（diff 链）。

本文目的：把 Hy Memory 中 memrec **尚未具备、且契合 memrec 定位**的能力识别出来，给出可执行的设计与优先级，避免盲目照搬。

---

## 2. 现状盘点（memrec 已有）

| 能力 | memrec 载体 |
|---|---|
| 核心记忆实体 | `mr-common/src/types/memory.rs`（`Memory`、`MemoryType`、`MemorySource`、`MemoryScope`） |
| 混合检索 KNN + BM25 | `mr-ability/src/storage/hybrid_store.rs`、`tantivy_store.rs` |
| 融合与重排 | RRF 融合 + `mr-ability/src/search/mmr.rs` + `scorer.rs`（时间衰减、源权重） |
| 图关系与扩散 | `mr-common/src/types/edge.rs`（Similar/CausedBy/References/SameProject/SameSession/Contradicts/Evolves）、`mr-ability/src/edge/`、`propagation/` |
| 结构化主题 | `mr-common/src/types/facet.rs`（`MemoryFacet`）、`mr-ability/src/facet/` |
| 主题聚合层 L2 | `mr-common/src/types/scene.rs`（`Scene`，热度聚合）、`storage/scene_store.rs` |
| 双系统（System 2 类比） | `mr-ability/src/dream/`（gate/lock/processor + 4 阶段：跨项目、个人提炼、清理、向量重生成）+ `idle/` + `queue/` |
| 生命周期与遗忘 | `mr-ability/src/lifecycle/`、`importance/`、软删除与恢复 |
| 原始存档 L0 | `mr-ability/src/archive/`（SQLite，按年切分，永不删除） |
| 分层呈现 | `mr-common/src/types/tiered.rs`（Full/Truncated/Summary/DenseProxy + token 预算） |
| 规则引擎 | `mr-ability/src/rule/` |
| 去重 | `mr-ability/src/dedup/` |

结论：Hy Memory 的检索类能力（混合检索、重排、冲突边、图库、场景聚合）memrec 基本已有甚至更细，**不值得吸纳**。差距集中在记忆形态。

---

## 3. 差距对照矩阵

| Hy Memory 能力 | memrec 现状 | 差距判定 |
|---|---|---|
| 双系统 System1/System2 | 实时链路 + `dream/` | 等价 |
| 混合检索 KNN+BM25+RRF | `hybrid_store` + RRF | 等价 |
| MMR 重排 | `search/mmr.rs` | 等价 |
| 图关系 Contradicts/Evolves | `edge/` + `propagation/` | 等价（memrec 甚至更强） |
| 场景聚合 | `Scene`（L2 热度） | 等价 |
| 任务域路由 chat/working/coding | 无此轴 | **缺失** |
| 双层记忆 Fact + Experience | 无结构化可复用经验层 | **缺失** |
| Coding 版本演进链（diff 链） | 无 | **缺失（最契合）** |
| 前瞻意图 L7（带有效期） | 无 | **缺失** |
| 原子事实版本化 + 冲突消解链 | `dedup/` + `Contradicts/Evolves` 边，非不可变版本链 | **部分** |
| 成本分级 lite/pro/ultra | 无显式分级 | 缺失 |
| 宿主来源隔离（openclaw/cursor/codex） | `MemorySource` 为可信度维度 | 语义不同 |
| 多后端（Chroma/Qdrant/Neo4j） | 固定 RocksDB+usearch+tantivy | 定位不同 |
| MemHub 跨应用 Hub | `dsh-plugin/`、MCP 集成进行中 | 理念一致 |
| cross-encoder rerank | 无 | 缺失（次要） |

---

## 4. 吸纳原则

为避免引入与现有架构冲突或语义重叠的概念，确定三条原则：

1. **装饰器而非替代器**。新增能力以"挂在既有 `Memory` 之上的附加层"形式存在，不改变 `Memory` 的核心地位与既有检索链路。演进链、经验层均遵循此原则。
2. **零 LLM 优先**。能用确定性数据（diff、坐标、时间戳）表达的，不调用 LLM；LLM 仅用于确有语义必要处，且受成本分级约束。
3. **正交且不重名**。新增维度与现有维度正交，命名必须区分。特别地，Hy Memory 的 `scene`（任务域）与 memrec 的 `Scene`（L2 主题聚合）**同名不同义**，新概念一律命名为 `TaskKind` / `Domain`，禁止复用 `Scene` 一词。

---

## 5. 上策（高价值，契合编码记忆定位）

### 5.1 Coding 版本演进链（diff 链）

**问题**：事实抽取刻意不粘贴代码（token 预算留给"文件 / 函数 / 分支"坐标），于是被回滚的那一版代码既不在工作区也不在 HEAD，事后无法读回。这是摘要层必然丢失的信息。

**机制**：以零 LLM 方式，把每处代码的历次真实修改（用户原话 + 真实 diff）按"链"持久化，并**作为事实的装饰器**：向量层负责决定返回哪些事实，链只负责在每条事实下挂上它涉及的代码全部修订版本。链本身不做检索。

**数据形态（概念）**：一条 chain 对应"某分支 + 某文件 + 某函数 + 某代码块"，隔离键为 `project_id`（对应 Hy 的 `workspace_id`）。每个 chain 由若干 revision 组成，每个 revision 记录：用户当时的表述、修改来源、真实 diff、时间戳、所属事实 ID。

**修改来源**两条路径，均为确定性：

- 本地 Git diff：配置仓库根后，按工具轨迹的时间点抓取对应 diff。
- 工具轨迹中的确定性替换动作：识别 `path / old / new` 形态的编辑动作，直接构造 diff。

**落点**：

| 层 | 载体 | 说明 |
|---|---|---|
| 类型 | `mr-common/src/types/coding.rs`（新增） | `CodeChain`、`CodeRevision`、代码坐标 |
| 能力 | `mr-ability/src/coding/chain.rs`（新增） | 抽取与追加修订、按事实挂载 |
| 存储 | `rocksdb.rs` 新增 CF：`code_chains`、`by_code_anchor`、`revisions` | 键为锚点/链 ID，值为 JSON |
| 接口 | `mr-protocol` 新增 `chain_block` 返回通道 | 检索结果附带的版本上下文 |
| 生命周期 | 挂入 `dream/` 清理阶段 | 过期链随记忆生命周期治理 |

**收益**：填补 memrec 相对 Hy Memory 的硬缺口，且是"编码 Agent 记忆"的差异化价值；零 LLM 成本。

### 5.2 前瞻意图层（Prospective Intention）

**问题**：memrec 无"未来要做的事"这一记忆形态，"待办 / 未决事项 / 后续行动"只能塞进普通记忆，无法按时间浮现、无法自动失效。

**机制**：新增一种带**有效期**的记忆形态。到期后自动降级为普通事实（或标记失效），由 `dream/` 定时扫描执行降级。

**落点**：

- `MemoryType` 之外新增意图维度（不污染现有五类），或新增 `Intention` 类型并附 `expire_at`。
- `dream/` 增加"到期意图扫描"阶段，与现有 4 阶段并列。
- 检索侧可选新增"主动通道"（proactive），把临近到期的意图排在结果前部。

### 5.3 可复用经验层（Experience）

**问题**：memrec 有 `facet`（主题）但无"可复用的做法"。做事场景中，"下次遇到类似任务可直接复用的方案"无处安放。

**机制**：结构化经验对象，含任务描述、检索键、解决方案、边界环境、置信度；且**必须通过价值门槛**才写入（每次会话 0–N 条），与"做过的事一定记得"的事实侧（无需门槛）互补。两层互相链接，支持溯源。

**落点**：

- `mr-common/src/types/experience.rs`（新增）：`Experience`。
- `mr-ability/src/experience/`（新增）：门槛判定、与事实/记忆的双向链接。
- 复用现有 `embedding` 与 `search`：经验作为独立可检索集合，与 `facet` 并列而非替代。

### 5.4 原子事实版本化与冲突消解链

**问题**：memrec 用 `dedup` + `Contradicts/Evolves` 边表达冲突与演进，但事实本身是可变的，缺少"不可变版本 + 版本链 + 被取代可追溯"。

**机制**：事实一经写入不可变；新事实与旧事实冲突时，不覆盖而是追加版本并建立 superseded 关系；对外提供"任务进展链摘要"，回溯演化过程。

**落点**：

- 现有 `Consolidation`/生命周期之上增加版本语义；`Contradicts`/`Evolves` 边承担链的显式表达。
- 与 5.1 的 diff 链天然协同（代码链是事实版本链在代码维度上的投影）。

---

## 6. 中策（增强现有能力）

| 项 | 说明 | 落点 |
|---|---|---|
| 成本分级 lite/pro/ultra | 按"是否含工具调用"决定是否走 LLM 抽取/分类，显式控制成本 | `mr-common/src/types/config.rs` 新增 `ProcessingMode`；写入入口分流 |
| 任务域轴 chat/working/coding | 与 `MemoryType` 正交的任务域分类，驱动 pipeline 分流 | 新增 `TaskKind`（**不得复用 `Scene`**） |
| 宿主来源隔离 | `source="cursor"/"codex"` + 检索 `include_sources/exclude_sources`，与可信度维度解耦 | `Memory` 增宿主来源字段或 `metadata` 约定 + 检索过滤 |
| 检索返回通道 + schema 配额 | profile/normal/proactive 三路，结合现有 `tiered` token 预算 | `tiered` 与检索结果结构扩展 |
| cross-encoder rerank | 可选精排后端 | `search/` 增加可选 rerank 阶段 |

---

## 7. 下策与不采纳

| 项 | 判定 | 理由 |
|---|---|---|
| 多向量库/图库后端（Chroma/Qdrant/FAISS/Neo4j） | 暂缓 | memrec 定位本地优先，抽象成本高、收益低 |
| Hy-Memory-30B-A3B 的 RL 写入模型 | 不采纳 | 超出 Rust 本地系统边界，依赖外部训练 |
| per-call embedding 端点覆盖 | 按需 | 小功能，非架构级 |
| 照搬 Hy 的 `scene` 命名 | 禁止 | 与 memrec 既有 `Scene`（L2 主题）语义冲突 |

---

## 8. 实施路线

**阶段一（代码版本演进链）**：独立、零 LLM、价值最高。先落类型与存储 CF，再接入写入轨迹抽取，最后加检索返回通道。

**阶段二（经验层 + 意图层）**：两个新增记忆形态，可并行。经验层复用现有向量检索；意图层复用 `dream` 调度与新增主动通道。

**阶段三（事实版本化 + 成本分级 + 任务域轴）**：改造面较大，需回归验证既有 `dedup`、生命周期与检索链路。

每阶段完成后执行 `cargo test --release`、`cargo clippy --release`、`cargo fmt --check`。

---

## 8.5 接入点可行性核对（2026-10-17 补充）

在设计上策第 1 项时，对照 memrec 实际代码逐点核对，发现**最大工作量不在存储，而在写入契约**：

1. **写入契约缺口（关键）**。`mr-protocol/src/request.rs` 的 `AddParams` 只有 `content / memory_type / tags / project_id / is_global / working_dir / source / scope / dedup`，`mr-protocol` 的 `SearchMemoryParams` 同理，**均无 messages / tool_calls / 代码坐标 / branch**。`memrecd` 的 `handle_add`（`server/handler.rs`）也是 content 字符串直入。因此 Hy Memory 的"带工具轨迹的写入"在 memrec 没有对应入口。吸纳 Coding 双层与 diff 链的前提，是**新增 trace 级写入契约**（一次会话 = 一条 trace），而非只加存储列族。这是本项真正的成本所在。
2. **事实可变性已确证**。`handle_add` 的去重分支 `DedupAction::Update` 会就地覆盖旧记忆的 `content` 并重建向量（`handler.rs`）。这正好印证 5.4 的缺口：当前事实是可变的，没有版本链。该分支同时也是版本链的**天然挂载点**。
3. **Dream 阶段框架可复用**。`mr-ability/src/dream/phases/` 已抽象出稳定的阶段模式（`PhaseResult` + four-phase），新增"意图到期扫描""链清理"阶段结构成本低，风险主要在单次 Dream 时长。
4. **列族扩展模式成熟**。`rocksdb.rs` 已有 `CF_SCENES / CF_BY_SCENE / CF_BY_HEAT / CF_BY_THEME` 四个场景列族先例，coding 链的 `code_chains / by_code_anchor / revisions` 可照搬同一模式。
5. **对外暴露成本低**。`memrec/src/mcp/mod.rs` 已提供 6 个工具与 resources 机制，新增 `chain_block` 返回通道与工具属增量改动。
6. **trace 级写入应优先落在插件侧**。`dsh-plugin/` 是当前唯一携带会话上下文（`working_dir`、session event）的入口，最接近 Hy 的 trace 语义；建议 trace 写入契约先在插件侧成形，再下沉到 `mr-protocol`。

结论：上策第 1 项应拆为"**先定 trace 写入契约 → 再落存储与抽取 → 最后接检索通道**"三步，且第一步是设计瓶颈，须先评审契约再动代码。

---

## 9. 风险与兼容性

- **兼容性**：新增 CF 与新增类型均以 `#[serde(default)]` 方式增量扩展，旧数据反序列化不受影响；`Memory` 现有字段保持不变。
- **命名冲突**：`TaskKind` 与 `Scene` 严格分离，文档与代码注释需明确。
- **成本控制**：经验层不带门槛会引入噪声与 token 膨胀，必须实现价值门槛。
- **Dream 负担**：意图到期扫描与链清理挂入 `DreamProcessor`，需评估单次 Dream 时长，避免阶段过多导致超时。

---

## 10. 参考资料

- Hy Memory README（Chat/Working/Coding 三场景、L0–L7 分层、lite/pro/ultra 模式）
- 论文：Memory Beyond Recall: A Dual-Process Cognitive Memory System for Self-Evolving LLM Agents（arXiv:2606.09483，EMNLP 2026）
- 论文：Hy-Memory-30B-A3B: Attributed Reinforcement Learning for Long-Term Memory Writing（arXiv:2610.22196）
- 基准：CMB: A Coding Memory Benchmark for Long-Horizon and Interleaved Coding Agents（ICLR 2027 投稿）
- memrec 设计：`docs/superpowers/specs/2026-04-23-memrec-design.md`、`docs/design/L2-scene-aggregation-design.md`
