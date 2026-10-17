# Dream 重构方案设计（D1 质量 / D2 治理 / D3 成网）

> 文档定位：基于"Hy Memory 2 对 memrec 的可吸收价值收敛于 dream 这一唯一主线"的结论，
> 对 memrec 现有 Dream 机制给出重构方案设计。
>
> 关联文档：`docs/design/hy-memory-absorption-design.md`、`docs/design/memhub-absorption-design.md`。
>
> 本文档为设计文档，不以代码为主体；仅用伪代码/结构示意澄清逻辑。

---

## 一、背景与目标

### 1.1 结论回顾

经前序分析，Hy Memory 2 对 memrec 的可吸收价值最终收敛为一句话：

> **记录的归记录（零 LLM），智能的归 dream（融合、成网、抽象）。**

其中：
- 记录路径保持零 LLM，memrec 已做到；
- 记忆融合才需要 LLM，memrec 已做到；
- 记忆分类（类型固定、标签灵活）不是杠杆，"机制如何发挥"才是；
- 抽象阶梯、记忆成网、经验提炼本质都是 dream 的产出形态，不应作为独立架构提案。

### 1.2 三个目标

| 代号 | 目标 | 一句话 |
|---|---|---|
| **D1** | 融合质量 | dream 产出的记忆更好：不串味、不丢数据线、能消解矛盾 |
| **D2** | 治理 | dream 可知、可查、可回溯、可度量、可预演 |
| **D3** | 成网 | dream 让记忆真正结成图，并让检索用上图 |

### 1.3 设计原则

1. **不新增平行记忆体系**：只重构 dream 内部，不引入"经验库/意图库"等并列大类。
2. **记录零 LLM 不变**：LLM 仅用于融合与成网两个后台阶段。
3. **复用既有机制**：图存储、传播、tiered 的 EdgeHint、SQLite archive 均已存在，优先复活而非新建。
4. **命名正交**：不得复用 `Scene`（memrec 中 `Scene` 已是 L2 主题聚合）。新概念命名：`DreamRun`、`LlmEdgeGenerator`、`EdgeBuilder`、`DreamReport`。
5. **幂等与可回溯优先**：任何重构不得破坏现有"整合幂等"（`metadata.source_ids`）与源记忆可恢复性。

---

## 二、现状核实

以下为对现有实现的核实结论，是后续设计的事实基础。

### 2.1 触发与调度（完备，无需重构）

| 环节 | 现状 | 位置 |
|---|---|---|
| 空闲触发 | 每 300s 读 `/proc/loadavg`，低于阈值发送 `DreamRequest` | `mr-ability/src/idle/monitor.rs` |
| 手动触发 | CLI `mr dream --force` → RPC `RequestAction::Dream` | `memrec/src/commands/memory.rs`、`memrecd/.../handler.rs` |
| 执行循环 | `dream_loop` 串行消费 `mpsc`，调用 `processor.execute(force)` | `memrecd/src/daemon/mod.rs` |
| 门控 | `DreamGate`：enabled / requires_llm / time / new-memories 四态 | `mr-ability/src/dream/gate.rs` |
| 并发锁 | `DreamLock`：`dream.lock`（PID，含僵锁检测）+ `dream.state`（last_run_at） | `mr-ability/src/dream/lock.rs` |

结论：调度层健壮，本次重构**不改动**触发/门控/锁的语义，仅在门控后追加"审计落库"。

### 2.2 融合核心

`process_inner` 的实际流程：

1. 全量扫描 `list(usize::MAX)`；
2. 过滤：未删除、`created_at < now - max_age_hours`、`importance < preserve_importance(0.85)`、不含保留标签（`critical`）、非保留类型（`preference`/`context`）、内容长度 ≥ `integration_min_content_len(10)`；
3. 排除已整合源（读 `dream-integrated` 记忆的 `metadata.source_ids`，崩溃残留补删）；
4. 按 `importance` **升序**排序，截取 `integration_max_memories(100)`；
5. 数量 < `min_memories(20)` 直接返回；
6. **一次** LLM 调用，将最多 100 条记忆整合为**单条** Markdown 摘要（主题/要点/结论模板）；
7. 写入 1 条整合记忆（`project_id = nil`、`scope = Global`、`source = System`、`importance = 0.8`、标签 `dream-integrated`，`metadata` 记录 `source_ids`/`source_details`/计数/重要性区间）；
8. **软删全部候选源记忆**；
9. 执行 Phase 1–4。

### 2.3 四阶段实现

| 阶段 | 实现 | 是否用 LLM | 产出 |
|---|---|---|---|
| Phase 1 CrossProject | 统计各 tag 出现在多少个 project | 否 | 一条 `cross-project` System 摘要 |
| Phase 2 PersonalSummary | 统计 type/tag 分布 | 否 | 一条 `personal-summary` System 摘要 |
| Phase 3 Cleanup | 删过期低重要性记忆；收敛重复摘要 | 否 | 删除计数 |
| Phase 4 VectorRegen | 为缺向量记忆重生成嵌入 | 否（用嵌入模型） | 向量写入 |

结论：**Phase 1/2 是确定性统计，不是认知提炼**。产出的是"标签清单"，信息量低，且 `source = System` 检索权重低，价值有限。

### 2.4 图机制：齐备但生产空转

这是本次核实最重要的发现。

| 能力 | 现状 |
|---|---|
| 边类型 | 7 类齐全（`SameProject`/`Similar`/`CausedBy`/`References`/`Contradicts`/`Evolves` 等） |
| 边存储 | `EdgeStore` 已实现 `EdgeStorage`，含 `edges`/`node_edges_out`/`node_edges_in` 三个 CF |
| 图遍历 | `traverse` 已实现带衰减、带类型权重的 BFS |
| 传播存储 | `PropagationStore` 已实现 `PropagationStorage`，含 `propagation_deltas`/`pending`/`applied` CF |
| 传播引擎 | `PropagationEngine` 已实现（邻居 → 增量 → pending/applied） |
| **生产生成器** | **缺失**：`EdgeGenerator` 仅 `MockEdgeGenerator`（测试）与 `ProjectEdgeGenerator`（仅 SameProject） |
| **生产接入** | **缺失**：`memrecd/src` 无任何 `EdgeStore`/`PropagationEngine`/`Generator` 构造或调用 |
| **dream 产边** | **无**：Phase 1–4 均不产生任何边 |

> 一句话：**图机制的"零件"都造好了，但整条生产线从未通电。** 记忆图目前事实上不存在。

### 2.5 治理与可观测：薄弱

| 缺口 | 现状 |
|---|---|
| 运行记录 | `DreamResult` 生成后仅写入内存并返回，**不落库**；跑完即丢 |
| 协议裁剪 | `handle_dream` 映射 `DreamResult` 时**丢弃** `phase_results`、`summary_clues` |
| 预演 | 无 dry-run，无法预知"将整合/删除哪些记忆" |
| 查询 | 无 dream history / report 查询入口 |
| 质量度量 | 无（压缩比、数据线保留率、phase 成功率均未统计） |
| 已知缺陷 | `extract_summary_clues` 中 `file_paths` 与 `line_numbers` 两段为**空的死代码**（注释声称"已收集"，实则永无写入），导致线索统计长期为空 |

### 2.6 问题小结

| 维度 | 核心问题 |
|---|---|
| D1 质量 | 单 blob 整合、主题串味、细节丢失、无冲突消解、Phase 1/2 无认知价值 |
| D2 治理 | 不可见、不可查、不可预演、不可度量、协议裁剪 |
| D3 成网 | 机制空转、dream 不建图、检索不走图 |

---

## 三、D1 融合质量

### 3.1 问题

1. **一把抓**：最多 100 条不同主题记忆塞进一次 LLM 调用，跨主题"串味"，细节与关键数据线易被概括掉。
2. **单 blob**：产出只有一条摘要，事实与综述混在一起，检索时无法按"原子事实"粒度命中。
3. **来源脱链**：源记忆被软删后退出语义检索路径；细节仅存于 SQLite archive（不进检索）。
4. **无冲突消解**：整合时不识别相互矛盾的记忆，矛盾被"揉进"摘要而消失。
5. **Phase 1/2 无效**：纯统计产出，形同鸡肋，反而增加摘要污染面（需靠 Phase 3 兜底收敛）。

### 3.2 目标

- 整合"**先分簇、再提炼**"，降低串味；
- 产出"**事实 + 摘要**"两层，提升检索粒度与保真度；
- 簇内**识别矛盾**，以显式关系而非含糊文字表达；
- 保留**可追溯**的来源指针；
- 让 Phase 1/2 从"统计"升格为"归纳"，或并入融合段。

### 3.3 方案

**D1-1 聚类后分主题融合**
利用已有向量存储对候选记忆做语义聚类，每个簇单独调用 LLM，避免跨主题混合。
簇数受控（例如按预算合并小簇），保证单次 LLM 输入聚焦。

**D1-2 分层产出**
一次融合产出两类记忆，作为 dream 的"产出形态"，而非新增记忆大类：
- **事实条（fact）**：去重后的原子事实，强制保留数据线（数字/路径/行号/术语）；
- **主题摘要（summary）**：跨记忆的关联、规律、后续行动。
两者的类型复用现有 `MemoryType`（`knowledge` 等），以标签区分（如 `dream-fact` / `dream-summary`），维持"类型固定、标签灵活"。

**D1-3 簇内冲突消解**
在簇内提示 LLM 标注矛盾对；矛盾不写进摘要正文，而是转成 `Contradicts` 边（见 D3）。摘要中以"存在分歧/待确认"一句带过并指向边。

**D1-4 来源可追溯（provenance）**
整合记忆保留 `source_ids`（已有），并建议新增整数型可读指针（如 `dream_run_id`）。检索结果可展开来源；原文细节以 archive 为最终兜底。
不要求把软删源恢复进检索，避免污染。

**D1-5 修复线索提取**
修复 `extract_summary_clues` 的两处空循环，使其真正收集 `file_paths`、`line_numbers`，作为质量度量输入（见 D2-3）。

**D1-6 Phase 1/2 重构**
两种路径，二选一：
- **路径 A（推荐）**：把 Phase 1/2 降级为"确定性分组信号"——为融合聚类提供"同主题/跨项目"提示，LLM 归纳统一在融合段完成，消除重复的 System 摘要；
- **路径 B**：保留两阶段，但把统计升级为 LLM 归纳（跨项目规律、用户画像）。成本更高，收益边际。

### 3.4 落点

| 项 | 触及模块 |
|---|---|
| D1-1/D1-2/D1-3 | `mr-ability/src/dream/processor.rs`（融合主流程）、可能的 `dream/fusion/` 新子模块 |
| D1-3 | 复用 `mr-common` `EdgeType::Contradicts` |
| D1-4 | `DreamResult`、协议 `DreamResult`、检索结果结构 |
| D1-5 | `processor.rs::extract_summary_clues` |
| D1-6 | `dream/phases/*` |

### 3.5 收益

- 检索粒度更细、保真更高（事实条可独立命中）；
- 串味与丢数据线显著下降；
- 矛盾从"被抹平"变为"被显式记录"，可被检索与人工复核。

---

## 四、D2 治理

### 4.1 问题

不可见（结果不落库）、不可查（无查询入口）、不可预演（无 dry-run）、不可度量（无线索保留率等）、协议裁剪（phase_results 等被丢弃）。

### 4.2 目标

让每一次 dream 都留下可查询、可度量的审计记录；支持预演；把质量从"感觉"变成"数字"。

### 4.3 方案

**D2-1 DreamRun 审计落库**
每次运行产出一条 `DreamRun` 记录，字段建议：

- `run_id`、`started_at`、`finished_at`、`duration_ms`、`force`
- `gate_outcome`（Allowed/TooSoon/…）
- `candidate_count`、`integrated_memory_ids`、`deleted_source_ids`
- `phase_results`（各阶段 success/processed/created/error）
- `summary`、`summary_clues`
- `llm_calls`、`llm_tokens`、`llm_latency_ms`
- `edge_count`、`propagation_count`、`contradiction_count`（D3 落地后填充）

存储位置二选一：RocksDB 新 CF（`CF_DREAM_RUNS`，与现有 CF 体系一致）或复用 SQLite archive（便于按时间分页/导出）。建议新 CF，读取走 RPC。

**D2-2 dry-run 预演**
新增 `dry_run` 参数：只计算"将整合的候选集合、将删除的源、拟产出摘要（可选，按配置决定是否真调 LLM）"，不写库、不删数据，返回预览。

**D2-3 质量度量**
随 DreamRun 记录：线索保留率（输出保留的数据线 / 输入数据线）、压缩比（源总字数 / 摘要字数）、矛盾数、边数、phase 成功率。可随时间趋势化，成为调参依据。

**D2-4 dream report/history 入口**
新增 RPC `RequestAction::DreamReport`，CLI `memrec dream report --last N`、MCP 对应工具，返回最近 N 次运行的审计摘要。

**D2-5 失败可诊断**
LLM 失败时保留候选集快照与错误分类，保证可重试；不刷新 `last_run_at`（现有语义保持）。

**D2-6 运行时可覆盖**
支持在一次调用中覆盖 `integration_max_memories`、`min_memories`、`dry_run` 等（不改持久配置）。

**D2-7 修复线索死代码**
见 D1-5，一并纳入治理度量。

### 4.4 落点

`mr-common`（`DreamConfig` 增字段、新 `DreamRun` 类型）、`mr-protocol`（`RequestAction::DreamReport`、`DreamParams` 增 `dry_run`、`ResponseResult` 增报表）、`mr-ability`（processing 落库、report 查询）、`memrecd`（`handle_dream` 返回完整结果、`handle_dream_report`）、`memrec`（CLI）。

### 4.5 收益

dream 从"黑盒后台任务"变为"可观测后台任务"，为后续所有调优提供数据基础。且本项**不改动核心语义，风险最低**，适合作为第一阶段。

---

## 五、D3 成网

### 5.1 问题

图存储、遍历、传播均已实现，但无生产生成器、dream 不产边、检索不走图，导致整条链路空转。

### 5.2 目标

让 dream 成为**建图阶段**，用 LLM 在 dream 中约束生成语义边，先以零 LLM 边让图"有内容"，最终让检索走上图。

### 5.3 关键判断：建图放在 dream，而非 add

- **add 时建边**：逐条触发、昂贵、噪声大、缺乏全局视野；
- **dream 时建边**：批量、分簇、时机自然（dream 本就在把记忆聚合成簇）；
- **收益现成**：边一旦真正写入，既有 `traverse` 与 `PropagationEngine` **立即复活**，无需新建基础设施。

这与"记忆成网是 dream 过程的产物"完全一致。

### 5.4 方案

**D3-1 生产边生成器**
新增 `LlmEdgeGenerator`（实现现有 `EdgeGenerator` trait），在 dream 分簇后对同簇记忆判定语义边：`CausedBy`/`Contradicts`/`Evolves`/`References`。
另有两类**零 LLM** 边先落地，让图先"有内容"：
- `Similar`：由已有向量近邻（阈值 + 每节点上限）生成；
- `SameProject`：复用 `ProjectEdgeGenerator`。

**D3-2 建图阶段（Stage Networking）**
在融合（Stage Fusion）之后插入建图：
- 对本次簇内成员建语义边；
- 对"整合摘要 ↔ 源记忆"建 `References`（provenance 边）。
需处理"边指向已软删源"的语义：**provenance 边保留**，检索扩散时跳过已删节点。

**D3-3 传播接入**
融合完成后用 `PropagationEngine` 把重要性/置信度增量传播到邻居（pending → applied）。

**D3-4 检索走图**
hybrid 检索得到 top-k 种子后，做 1 跳 `traverse`（带类型权重与衰减），命中并入候选，交 RRF/rerank。tiered 已有 `EdgeHint` 接口可复用。

**D3-5 边生命周期治理**
在 Phase 3（Cleanup）中扩展边治理：清理悬挂边（目标已硬删）、过期/低置信边、矛盾边收敛。

**D3-6 daemon 装配**
在 `memrecd` 中构造 `EdgeStore`/`PropagationStore`（复用同一 `RocksDBStore`），注入 `DreamProcessor` 与 `Router`。
注意：`EdgeStore::new` 现签名为按值接收 `RocksDBStore`，而 `MemoryStore`/`SceneStore`/`PropagationStore` 均以 `Arc<RocksDBStore>` 共享同一实例；装配时需将 `EdgeStore` 对齐为 `Arc<RocksDBStore>`，否则会出现多实例打开同一路径的问题。

### 5.5 落点

`mr-ability/src/edge/generator.rs`（新增 `LlmEdgeGenerator`）、`dream/processor.rs`（Stage Networking）、`mr-ability/src/propagation/`（复用）、`mr-ability/src/search/`（图扩散并入）、`mr-ability/src/storage/edge_store.rs`（`new` 签名对齐）、`memrecd/src/daemon/mod.rs`（装配）。

### 5.6 收益

- 记忆图从"空转机制"变为"真实网络"；
- 检索可经图扩散命中"向量/关键词都够不着"的关联记忆；
- 矛盾/演进关系可被查询，为后续推理与复核提供结构。

---

## 六、上中下三策

### 6.1 上策：融合-成网两段式完整重构

把 dream 重构为两段式管线：

- **Stage Fusion**：聚类 → 分主题 LLM（事实 + 摘要 + 矛盾）→ 写记忆；
- **Stage Networking**：零 LLM 边 + LLM 语义边 → 传播 → 边治理；
- 叠加 **D2 全套**：DreamRun 审计 + dry-run + report + 质量度量。

覆盖 D1/D2/D3 全部目标。工作量大，触及存储、协议、检索、daemon 装配。适合作为方向蓝图，分阶段落地。

> 上策的完整设计见 `docs/design/dream-refactor-strategy-a.md`（两段式融合与成网的完整蓝图）。

### 6.2 中策：分步见效（推荐）

按"先可观测、后建图、再提质"推进，每步独立可验证：

1. **P0 治理与修复**：DreamRun 落库、report 入口、修复 clue 死代码、handler 返回完整 `phase_results`；
2. **P1 零 LLM 建图**：`Similar`（向量近邻）+ `SameProject` 边 + daemon 装配 + 检索走图；
3. **P2 治理增强**：dry-run + 质量度量；
4. **P3 质量重构**：聚类分主题融合 + 事实/摘要分层；
5. **P4 语义成网**：`LlmEdgeGenerator` 语义边 + 传播 + 矛盾消解 + 边治理。

风险可控，每阶段都有独立收益与回退点。**推荐采纳。**

### 6.3 下策：最小修复

只做三件事：
- DreamRun 审计落库（D2-1）；
- 修复 clue 死代码（D2-7）；
- 接入 `ProjectEdgeGenerator`（`SameProject` 零 LLM 边），让图从"零"变"非零"。

不改检索、不改协议语义。低风险，收益有限。

### 6.4 三策对比

| 维度 | 上策 | 中策（推荐） | 下策 |
|---|---|---|---|
| D1 质量 | 完整 | 分阶段达成 | 不触及 |
| D2 治理 | 完整 | 完整（P0/P2） | 仅审计+修复 |
| D3 成网 | 完整 | 零 LLM 先行，语义边后置 | 仅 SameProject |
| 风险 | 高 | 中 | 低 |
| 交付节奏 | 一次性 | 持续逐段 | 一次性 |
| 建议 | 蓝图 | **采纳** | 兜底 |

---

## 七、路线图（中策展开）

| 阶段 | 内容 | 依赖 | 独立可验证点 |
|---|---|---|---|
| P0 | DreamRun 落库、report、修复 clue、协议返回完整结果 | 无 | `dream report` 可查历史与线索 |
| P1 | `Similar` + `SameProject` 建图、daemon 装配、检索走图 | P0（便于观测边数） | 图非空；检索可命中图扩散结果 |
| P2 | dry-run、质量度量（保留率/压缩比/矛盾数/边数） | P0 | 预演一致；度量随运行记录 |
| P3 | 聚类分主题融合、事实/摘要分层 | P1（复用向量近邻） | 串味下降；事实条可独立命中 |
| P4 | `LlmEdgeGenerator` 语义边、传播、边治理 | P1/P3 | 矛盾/演进边生成；传播增量生效 |

---

## 八、风险与非功能

| 维度 | 风险 | 缓解 |
|---|---|---|
| 性能 | LLM 分批调用、聚类成本 | dream 为空闲批处理，可限流分批；聚类复用已有向量，不重复嵌入 |
| 质量 | 建图噪声污染检索 | 置信度阈值 + 类型权重 + 每节点边上限 + 定期重建/衰减 |
| 兼容 | 新 CF、协议新字段 | 字段与 CF 均为增量，向后兼容；旧数据无需迁移 |
| 幂等 | 重构破坏 source_ids 幂等 | 保持 `dream-integrated` + `source_ids` 语义，回归测试覆盖 |
| 悬挂边 | 源软删/硬删后边悬空 | provenance 边保留、检索跳过已删节点；cleanup 清理硬删悬挂边 |
| 安全 | LLM 出网、审计含敏感 | 沿用现有 LLM 出网边界；审计不落原文，仅落 id 与统计 |
| 可用性 | 失败后长窗口重试 | 保持"失败不刷新 last_run_at"；保留候选快照 |

---

## 九、命名与接口影响

### 9.1 命名（正交，避免与 `Scene` 冲突）

| 新概念 | 含义 | 禁止用名 |
|---|---|---|
| `DreamRun` | 一次 dream 运行的审计记录 | 不可叫 Scene/Session |
| `LlmEdgeGenerator` | LLM 语义边生成器 | — |
| `EdgeBuilder` | dream 建图阶段协调者 | — |
| `DreamReport` | dream 历史/报表查询结果 | — |

### 9.2 接口影响

| 模块 | 影响 |
|---|---|
| `mr-common::types::DreamConfig` | 增 `dry_run` 等运行时可覆盖字段；新增 `DreamRun` 类型 |
| `mr-protocol` | `DreamParams` 增 `dry_run`；新增 `RequestAction::DreamReport`；`ResponseResult` 增 report |
| `mr-ability::storage` | 新增 `CF_DREAM_RUNS`（建议）；`EdgeStore::new` 对齐 `Arc<RocksDBStore>` |
| `mr-ability::dream` | 融合主流程改分簇；新增 Stage Networking |
| `mr-ability::edge` | 新增 `LlmEdgeGenerator` |
| `memrecd` | `handle_dream` 返回完整结果；装配 `EdgeStore`/`PropagationStore`；新增 report handler |
| `memrec` | 新增 `dream report` CLI 子命令 |

---

## 十、结论

D1/D2/D3 的重构可以统一为一条演进路径：**先让 dream 可知（D2），再让记忆成网（D3 零 LLM 先行），最后让融合更深（D1/D3 语义）**。

推荐采纳**中策**：以 P0 打通可观测为起点，P1 让图先"通电"，之后逐段提升质量与语义。整条路径不新增平行记忆体系、不破坏记录零 LLM 与整合幂等，且每一阶段都可独立验证与回退。
