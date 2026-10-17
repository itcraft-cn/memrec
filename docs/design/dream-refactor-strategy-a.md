# Dream 上策完整设计：两段式融合与成网

> 文档定位：`docs/design/dream-refactor-design.md` 的展开。前文给出 D1/D2/D3 三目标与上/中/下三策；
> 本文对**上策**做一次性完整设计（融合 + 成网两段式管线，叠加 DreamRun 治理），不再分阶段取舍。
>
> 本文为设计文档，不以代码为主体；关键处仅用结构示意与伪代码澄清逻辑。

---

## 一、上策总览

上策把 dream 重构为一条**两段式管线**：

```
记录路径（零 LLM，不变）
        │
        ▼
   [记忆/向量/全文存储] ── 空闲监控 / 手动触发 ──► Gate → Lock
        │
        ▼
┌─────────────────────────────────────────────────────────────┐
│ DreamRun（一次运行，全程审计）                                │
│                                                             │
│  Stage F —— 融合（Fusion）                                   │
│    候选筛选 → 语义聚类 → 分簇 LLM 提炼(事实/摘要/矛盾)        │
│             → 写入整合记忆 + provenance → 处置源记忆          │
│                                                             │
│  Stage N —— 成网（Networking）                               │
│    零 LLM 边(Similar/SameProject) → LLM 语义边                │
│    (CausedBy/Contradicts/Evolves/References)                 │
│    → 传播增量 → 边生命周期治理                                │
│                                                             │
│  收尾：写入 DreamRun 审计 + 质量度量                          │
└─────────────────────────────────────────────────────────────┘
        │
        ▼
   检索路径：hybrid(KNN+BM25) → 图扩散(1 跳) → RRF/MMR 重排 → 分层输出
```

一句话：**融合让记忆更精，成网让记忆更连，DreamRun 让过程可见。**

---

## 二、设计目标与不变量

### 2.1 目标

| 代号 | 目标 | 上策达成方式 |
|---|---|---|
| D1 质量 | 不串味、不丢数据线、能消解矛盾 | Stage F 聚类 + 分层产出 + 矛盾边 |
| D2 治理 | 可知、可查、可回溯、可度量、可预演 | DreamRun 落库 + dry-run + report + 度量 |
| D3 成网 | 记忆真正结网并用于检索 | Stage N 建图 + 传播 + 检索图扩散 |

### 2.2 不变量（不得破坏）

1. **记录路径零 LLM**：新增/更新记忆仍不调用 LLM。
2. **整合幂等**：同一批源记忆不得被重复整合；崩溃残留可识别补删。
3. **源可恢复**：源记忆的原文可经软删恢复或 archive 检索取得。
4. **失败不刷新时间窗**：LLM/存储失败时不得更新 `last_run_at`。
5. **命名正交**：不得复用 `Scene`；新概念一律用 `DreamRun` / `LlmEdgeGenerator` / `EdgeBuilder` / `DreamReport`。

---

## 三、数据模型

### 3.1 新增：DreamRun（一次运行的审计记录）

| 字段 | 类型 | 说明 |
|---|---|---|
| `run_id` | Uuid | 运行标识 |
| `started_at` / `finished_at` | DateTime | 起止时间 |
| `duration_ms` | u64 | 耗时 |
| `force` | bool | 是否强制触发 |
| `gate_outcome` | enum | Allowed / TooSoon / NotEnoughNew / Disabled / LlmNotConfigured |
| `status` | enum | Success / Failed / NoOp |
| `candidate_count` | usize | 参与候选数 |
| `cluster_count` | usize | 聚类簇数 |
| `integrated_memory_ids` | Vec<Uuid> | 本次写入的整合记忆（含事实条与摘要） |
| `summary_memory_id` | Option<Uuid> | 主题摘要记忆 |
| `deleted_source_ids` | Vec<Uuid> | 本次软删的源 |
| `phase_results` | Vec<PhaseResult> | 各阶段执行结果 |
| `edge_count` / `edge_counts_by_type` | usize / Map | 本次新建边数量与分布 |
| `propagation_count` | usize | 本次传播增量数 |
| `contradiction_count` | usize | 识别出的矛盾对数 |
| `llm_calls` / `llm_tokens` / `llm_latency_ms` | u64 | LLM 用量 |
| `clues` | SummaryClues | 摘要中的关键数据线 |
| `metrics` | QualityMetrics | 见 7.3 |

> `DreamRun` 是本设计的核心治理载体：D2 的全部能力（查询、预演对比、度量、失败诊断）都挂在它上面。

### 3.2 扩展：DreamConfig

在既有字段之上新增（均带 serde default，兼容旧配置）：

| 字段 | 默认 | 说明 |
|---|---|---|
| `fusion_cluster_enabled` | true | 是否启用语义聚类 |
| `fusion_cluster_threshold` | 0.78 | 聚类相似度阈值 |
| `fusion_cluster_max_size` | 20 | 单簇上限（超出拆分） |
| `fusion_max_clusters` | 12 | 单次最大簇数 |
| `fusion_emit_facts` | true | 是否产出事实条 |
| `fusion_emit_summary` | true | 是否产出主题摘要 |
| `fusion_detect_contradictions` | true | 是否做矛盾消解 |
| `networking_enabled` | true | 是否启用成网段 |
| `networking_similar_topk` | 5 | Similar 边近邻数 |
| `networking_similar_threshold` | 0.82 | Similar 边阈值 |
| `networking_semantic_enabled` | false | 是否启用 LLM 语义边（默认关，先零 LLM 跑图） |
| `networking_propagation_enabled` | true | 是否启用传播 |
| `retrieval_graph_enabled` | true | 检索是否走图扩散 |
| `retrieval_graph_depth` | 1 | 扩散跳数 |
| `retrieval_graph_decay` | 0.6 | 衰减 |
| `retrieval_graph_weight` | 0.3 | 图命中并入候选的权重 |

### 3.3 复用：边与传播

`EdgeType`（7 类）、`MemoryEdge`、`PropagationDelta`、`PropagationConfig` 全部复用，不新增类型。

### 3.4 provenance 指针

整合记忆（事实条与摘要）的 `metadata` 保留并新增：

- `source_ids`：本次簇内源记忆 id 列表（沿用，保幂等）；
- `source_details`：来源摘要（沿用）；
- `dream_run_id`：所属 DreamRun（新增，用于回溯）；
- `cluster_id`：所属簇号（新增，便于按簇重建图）。

---

## 四、Stage F —— 融合（Fusion）

### 4.1 候选筛选

基本沿用现流程（未删除、早于 `max_age_hours`、非高价值、内容长度足够、排除已整合），
但**不再**直接降为"单批"；筛选结果交给聚类。

调整点：
- 候选数不足 `min_memories` 时，正常返回 `DreamRun{status: NoOp}` 并落库（可观测"为何没做事"）。

### 4.2 语义聚类

目标：把候选按语义分成若干聚焦主题的簇，避免一次 LLM 输入混杂多主题。

算法（伪代码）：

```
embeddings = 对每个候选取已有向量（缺失则批量补算）
seeds = 按 importance 降序
for seed in seeds:
    if seed 已归属: continue
    cluster = [seed]
    for other in 未归属且非 seed:
        if cos(seed, other) >= threshold:
            cluster.append(other)
    if cluster.size > max_size: 按相似度拆分为多个子簇
    归属 cluster
若簇数 > max_clusters: 合并最小的若干簇
无向量可用时退化：按 project_id + memory_type 分组
```

特性：
- 零 LLM；
- 复用已有向量，无重复嵌入开销；
- 结果确定性（按 importance 顺序 + 阈值），可复现。

### 4.3 分簇提炼（每簇一次 LLM）

对每个簇调用 LLM，要求**结构化输出**：

```
输入：簇内记忆（内容 + 类型 + 标签 + 关键数据线提示）
输出（JSON）：
{
  "facts": [ {"text": "...", "source_hints": [行号/路径/数字]} ],
  "summary": "（跨记忆关联/规律/行动，≤500 字）",
  "contradictions": [ {"a": 0, "b": 2, "reason": "..."} ]
}
```

约束沿用以有的"禁套话、禁编造、保留数据线、语言一致"策略；新增：
- 事实条必须独立、可检索、保留原始表述；
- 矛盾只记录下标对与理由，不写进摘要正文。

解析失败回退：退化为现有 Markdown 摘要（主题/要点/结论），保证鲁棒。

### 4.4 产出写入

| 产出 | 记忆形态 | 标签 | 说明 |
|---|---|---|---|
| 事实条 | `MemoryType::Knowledge`，`source=System` | `dream-fact` + 运行标签 | 每簇多条，原子事实 |
| 主题摘要 | `MemoryType::Knowledge`（可配） | `dream-summary` + 运行标签 | 每簇一条 |
| 矛盾 | 不落记忆 | — | 转为 `Contradicts` 边（Stage N） |

要点：
- 事实条与摘要均写入 `dream_run_id` / `cluster_id` / `source_ids`；
- 类与标签遵循"类型固定、标签灵活"：不新增记忆大类，用标签表达 dream 产出形态；
- 保持 `dream-integrated` 运行标签，用于幂等与既有工具兼容。

### 4.5 源记忆处置

- 本次簇内源记忆**软删**（保持可恢复）；
- 幂等仍由"已整合源 id 集合"（读 `dream-integrated`/DreamRun 的 `source_ids`）保证，崩溃残留补删逻辑保留。

### 4.6 Phase 1/2 归并（路径 A）

现有 Phase 1（CrossProject 统计）/ Phase 2（PersonalSummary 统计）为纯统计，价值低。上策采用**路径 A**：

- 二者**降级为确定性分组信号**，为 4.2 聚类提供"同主题/跨项目"提示；
- 不再产出独立的 `cross-project` / `personal-summary` 摘要记忆（消除重复摘要污染面）；
- 跨项目规律与用户画像由 Stage F 的分簇摘要自然承载。

---

## 五、Stage N —— 成网（Networking）

### 5.1 零 LLM 边（先让图"有内容"）

| 边 | 生成方式 | 权重来源 |
|---|---|---|
| `Similar` | 对每条整合记忆取向量 top-k 近邻（阈值 + 每节点上限） | 余弦相似度 |
| `SameProject` | 复用 `ProjectEdgeGenerator`（同 project_id） | 固定 0.5 |

零 LLM 边**默认开启**，让图先非空，且不增加 LLM 成本。

### 5.2 LLM 语义边

新增 `LlmEdgeGenerator`（实现既有 `EdgeGenerator` trait）：

- 输入：簇内记忆集合（+ 摘要）；
- 输出：`CausedBy` / `Contradicts` / `Evolves` / `References` 边及置信度；
- 约束：每节点边数上限、置信度阈值下限、证据必填；
- 默认**关闭**（`networking_semantic_enabled=false`），零 LLM 验证后再开。

矛盾来源：4.3 的 `contradictions` 下标对直接转 `Contradicts` 边，无需二次 LLM。

### 5.3 provenance 边

- `摘要/事实条 → 源记忆`：`References`（有向）；
- 源记忆为软删节点，**边保留**；检索扩散时跳过已删节点（见第六节）。

### 5.4 传播

- 整合记忆写入后，调用 `PropagationEngine`：以边邻居为对象生成 `PropagationDelta`（`KnowledgeUpdate`/`Contradiction` 等）；
- 采用规则型 `DeltaGenerator`（零 LLM）：按边类型映射增量类型，置信度取边权重；
- pending → applied 流程复用现有实现。

### 5.5 边生命周期治理

在清理阶段（原 Phase 3）扩展：

| 项 | 处理 |
|---|---|
| 悬挂边 | 目标已被**硬删**（非软删）→ 删除该边 |
| 低置信边 | 权重低于阈值且陈旧 → 删除 |
| 重复边 | 同源同目标同类型去重 |
| 矛盾收敛 | 已由摘要承载的矛盾对保留边，供检索/复核 |

---

## 六、检索接入图扩散

### 6.1 流程

```
query ─► hybrid(KNN+BM25) ─► seed hits(top_k)
                                   │
                                   ▼
                     edge_storage.traverse(seed_ids, params)
                                   │
                    图命中(score, path, path_types)
                                   │
        ┌──────────────────────────┴───────────────────────┐
        ▼                                                   ▼
   seed 基础分                                        图命中折算分
        └──────────────────────► 融合(RRF/加权) ◄──────────┘
                                   │
                                   ▼
                              MMR 重排 → 分层输出
```

### 6.2 参数

`GraphTraversalParams{ max_depth=retrieval_graph_depth(1), decay=retrieval_graph_decay(0.6), min_score, max_results, edge_types=Some(Similar/References/CausedBy/Evolves，不含 SameProject 泛边) }`。

图命中分 = `seed_score × graph_score × retrieval_graph_weight`，并入候选后统一重排。

### 6.3 复用与过滤

- 复用 `tiered::EdgeHint` 表达"经图到达"的来源信息，供分层输出标注；
- 物化结果时过滤 `is_deleted` 记忆（软删源不返回，仅用于建图连通）。

---

## 七、治理子系统（D2）

### 7.1 DreamRun 落库

- 新增 CF `CF_DREAM_RUNS`（key=run_id，value=JSON）；
- 新增 `DreamRunStorage` trait：`save_run` / `get_run` / `list_runs(limit, offset)` / `count_runs`；
- 运行开始写入占位（status=Running），结束更新为终态（Success/Failed/NoOp）；失败也落库（区别于 `last_run_at`，后者失败不刷新）。

### 7.2 dry-run 预演

- `DreamParams.dry_run` 开启时：只执行候选筛选与聚类，产出"将整合的簇、将写入的记忆草案、将删除的源、将建的边草案"，**不写库、不删数据**；
- 是否真调 LLM 由配置决定（`dry_run_with_llm`）：关闭时用模板占位摘要；
- 返回一个不落库的 `DreamRun` 预览。

### 7.3 质量度量

随 DreamRun 记录：

| 指标 | 定义 |
|---|---|
| 线索保留率 | 摘要保留的关键数据线数 / 输入数据线数 |
| 压缩比 | 源总字数 / 产出总字数 |
| 簇数/平均簇大小 | 聚类质量 |
| 矛盾对数 | 冲突消解活跃度 |
| 边数/传播数 | 成网活跃度 |
| phase 成功率 | 各阶段健康度 |

### 7.4 DreamReport

- 新增 `RequestAction::DreamReport`；
- 返回最近 N 次 DreamRun 的审计摘要（含度量与 phase_results）；
- CLI `memrec dream report --last N [--json]`；MCP 对应工具。

### 7.5 失败诊断与协议完整返回

- `handle_dream` 不再裁剪：返回完整 `DreamResult`（含 phase_results / clues / run_id）；
- LLM 失败保留候选快照引用（记入 DreamRun），便于重试与定位。

### 7.6 修复线索提取

修复 `extract_summary_clues` 中 `file_paths`、`line_numbers` 两处空循环（当前为死代码，永远收集不到），使度量有效。

---

## 八、存储设计

### 8.1 CF 变更

| CF | 状态 | 用途 |
|---|---|---|
| `CF_DREAM_RUNS` | **新增** | DreamRun 审计 |
| `CF_EDGES` / `CF_NODE_EDGES_OUT` / `CF_NODE_EDGES_IN` | 已存在 | 边与邻接索引 |
| `CF_PROPAGATION_DELTAS` / `CF_PENDING_PROPAGATIONS` / `CF_APPLIED_PROPAGATIONS` | 已存在 | 传播 |

### 8.2 存储 trait/实现

| 项 | 说明 |
|---|---|
| `DreamRunStorage`（新） | `dream_run_store.rs`，基于 `RocksDBStore` |
| `EdgeStore`（已有） | 直接复用 |
| `PropagationStore`（已有） | 直接复用 |
| `EdgeStore::new` 签名 | 现为按值接收 `RocksDBStore`，与 `MemoryStore`/`SceneStore`/`PropagationStore` 的 `Arc<RocksDBStore>` 共享模式不一致；**对齐为 `Arc<RocksDBStore>`（或提供 `from_arc`）**，确保同一 DB 实例 |

---

## 九、协议与接口

### 9.1 协议变更

| 变更 | 内容 |
|---|---|
| `DreamParams` | 新增 `dry_run: bool`（serde default） |
| `RequestAction` | 新增 `DreamReport` |
| `ResponseResult` | `Dream` 结果扩展（run_id/metrics/phase_results/clues）；新增 `DreamReport` 结果 |
| `DreamReportParams` | 新增 `{ last: usize, include_metrics: bool }` |

向后兼容：旧客户端不传新字段时走默认；旧 `Dream` 字段全部保留。

### 9.2 CLI / MCP

- CLI：`memrec dream [--force] [--dry-run]`、`memrec dream report --last N [--json]`；
- MCP：新增 `mr_dream_report` 工具，`mr_dream` 增 `dry_run` 参数。

---

## 十、守护进程装配

装配顺序（在现有 `daemon/mod.rs` 基础上增补）：

```
rocksdb = Arc::new(RocksDBStore::open(data_dir))
memory_store  = MemoryStore::new(rocksdb.clone())
scene_store   = SceneStore::new(rocksdb.clone())
edge_store    = EdgeStore::new(rocksdb.clone())          // 对齐 Arc
prop_store    = PropagationStore::new(rocksdb.clone())
run_store     = DreamRunStore::new(rocksdb.clone())      // 新增

edge_generator   = LlmEdgeGenerator(可选) + ProjectEdgeGenerator(零LLM)
delta_generator  = RuleDeltaGenerator(零LLM)
prop_engine      = PropagationEngine::new(cfg, memory_store, edge_store, prop_store, delta_generator)

dream_processor = DreamProcessor::new(cfg, data_dir, memory_store)
    .with_vector_store(...)
    .with_embedder(...)
    .with_llm(...)
    .with_edge_storage(edge_store)          // 新增
    .with_propagation(prop_engine)          // 新增
    .with_edge_generator(edge_generator)    // 新增
    .with_run_store(run_store)              // 新增

Router 注入 edge_store（供检索图扩散）
```

---

## 十一、运行时序

**Stage F**：

```
筛选候选 → 聚类 → 逐簇 LLM(事实/摘要/矛盾)
  → 写事实条 + 摘要(带 provenance) → 软删源
```

**Stage N**：

```
零 LLM 边(Similar/SameProject) → [可选]LLM 语义边 + 矛盾边
  → provenance 边 → 传播增量(pending)
```

**收尾**：

```
清理(含边治理) → 写 DreamRun 终态 + 度量 → 释放锁 + 写 state
```

**检索**：

```
hybrid → traverse → 融合重排 → MMR → 分层
```

---

## 十二、迁移与兼容

| 项 | 策略 |
|---|---|
| 旧配置 | 新字段 serde default，缺失走默认 |
| 旧 DreamRun | 无历史记录，report 首跑为空 |
| 旧边与传播 | 图原本为空，无迁移负担；首跑 Stage N 后图逐步丰富 |
| 旧摘要（single blob） | 保留；新建记忆改用 fact/summary 标签，可共存 |
| 幂等 | 沿用 `source_ids`；DreamRun 记录源以增强可追溯 |
| 协议 | 纯增量，向后兼容 |
| 软删源上的边 | 保留；检索物化时跳过已删节点 |

---

## 十三、非功能与风险

| 维度 | 风险 | 缓解 |
|---|---|---|
| 性能 | 聚类 O(n²)（n≤100 上限可接受）、逐簇 LLM 调用增加 | 分批 + 间隔（沿用 batch 机制）；簇上限 |
| 成本 | 语义边/逐簇提炼消耗 LLM | 语义边默认关；空闲批处理；限量 |
| 质量 | LLM 结构化输出解析失败 | 回退 Markdown 摘要；JSON schema 校验 |
| 噪声 | 建图噪声污染检索 | 阈值 + 类型权重 + 每节点上限；图权重可调；定期治理 |
| 一致性 | 融合成功但成网失败 | 分段提交；DreamRun 记录阶段态；成网失败不回滚融合 |
| 兼容 | 新 CF/协议 | 纯增量 |
| 安全 | LLM 出网 | 沿用既有 LLM 边界；审计不落原文 |
| 可用性 | 失败后重试 | 失败不刷新 `last_run_at`；保留候选快照 |

---

## 十四、测试策略

| 层 | 用例 |
|---|---|
| 单元 | 聚类阈值/拆簇/合并；clue 提取（修复后 file_paths/line_numbers）；DreamRun 读写；dry-run 不改库 |
| 单元 | `LlmEdgeGenerator` 解析；`RuleDeltaGenerator` 映射；Similar 边近邻与阈值 |
| 集成 | 端到端：候选→聚类→写入事实/摘要→建边→传播→DreamRun 落库（LLM 用 Mock） |
| 集成 | 检索图扩散：构造边，验证经图命中的记忆进入结果并被标注 EdgeHint |
| 回归 | 幂等（重复运行不重复整合）；失败不刷新 state；软删源可恢复 |
| 兼容 | 旧 config 缺失字段；protocol 旧字段子集 |

---

## 十五、落地次序（内部参考，非交付取舍）

上策为一次性设计；实施时建议如下顺序以降低联动风险：

1. 基础：`DreamRun` 类型 + `CF_DREAM_RUNS` + `DreamRunStorage` + 修 clue；
2. Stage F：聚类 + 分簇提炼 + 分层写入 + provenance；
3. Stage N 零 LLM：`EdgeStore`/`PropagationStore` 装配 + Similar/SameProject 边 + 传播；
4. 检索：图扩散接入 + EdgeHint 标注；
5. 协议/CLI：`DreamReport` + `dry_run` + `handle_dream` 完整返回；
6. Stage N 语义：`LlmEdgeGenerator` + 矛盾边 + 边治理（默认关，灰度开启）。

---

## 十六、结论

上策用一条两段式管线同时解决 D1/D2/D3：

- **融合**（Stage F）解决"产出更好"：聚类分主题、事实与摘要分层、矛盾显式化；
- **成网**（Stage N）解决"记忆成网"：零 LLM 边先让图有内容，语义边后置增强，边一旦落库，既有 `traverse` 与 `propagation` 立即复活；
- **治理**（DreamRun）贯穿全程：可知、可查、可预演、可度量，并修复既有线索提取缺陷；
- **检索**接入图扩散，让"网"真正产生召回价值。

全程不新增平行记忆体系，不破坏记录零 LLM、整合幂等与源可恢复三条底线。该设计与 `docs/design/dream-refactor-design.md` 的中策路线图方向一致，是其中的"完整形态"。

---

## 附则 A：记录更新窗口与重要性口径（2026-10-17 修订）

### A.1 记录更新窗口

原设计隐含"记录路径不覆盖"，修订为**有界更新窗口**：

- 窗口 `W` 内（默认 30–60 分钟，配置项 `DedupConfig.update_window_mins`）：去重路径允许**原地 Update**（同一讨论语境，安全）；
- 窗口 `W` 外：自动路径**不得修正/删除历史**；新信息一律 Add，修正以**取代标记**表达；
- **合并与软删仅由 dream 执行**；
- 兜底：一切删除均为软删，原始记录留存于 RocksDB。

影响：`RuleDedupDecider` 增加时间判定，`Update` 决策仅在窗口内生效；窗口外的"修正"落为 Add 或 `superseded` 标记，交由 Stage F 融合消解。

### A.2 重要性口径

- 每记忆 `importance` 仅作**全局先验**，不承载"对某事物/项目的评分"——后者查询相关，**不可存储**（与实体的开放维度陷阱同类）；
- dream 产生的评级为**簇域（topic-scoped）**：附着于该簇产出的摘要/事实条，作用域由簇固定，定义清晰且不爆炸；**不写回为全库开放维度**；
- 检索分 = 全局 `importance` 先验 × 查询期相关度。

影响：§4.4 产出的重要性由"固定 0.8"改为"**全局基线 + 簇域相对评级**"；不新增每记忆的开放评级字段。
