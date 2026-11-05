# MemRec 检索与记忆质量改进计划

- 版本：2026-10-26
- 状态：待排期
- 关联：`AGENTS.md` 的「多步骤测试与部署」门禁流程

## 1. 背景

2026-10-26 对运行中的 MemRec 做了一轮实测，确认当前系统"存得准、取得不稳"。核心问题不是存储保真度，而是检索分数的构造方式。

实测数据（scope=all，min_score=0，top_k=5）：

| 查询 | top1 | top2 | top3 | 观察 |
|---|---|---|---|---|
| 完全无关的烘焙面包配方 | 0.500 | 0.317 | 0.271 | 仅"无关"二字命中 BM25，融合后仍得 0.5 |
| glibc arena chunk 偏移 malloc_state | 0.500 | 0.500 | 0.492 | 头部不可区分，排序接近噪声 |

代码定位：`mr-ability/src/storage/hybrid_store.rs` 的 `merge_and_normalize` 对向量与 BM25 分别在**本次结果集内做 min-max 归一化**，再按 alpha 加权。因此单路结果的 top 恒等于 `alpha`（纯向量）或 `1-alpha`（纯 FTS），默认均为 0.5，与绝对相关性无关。

## 2. 目标与非目标

### 目标

1. 让检索分数具备可解释的绝对语义，避免"单路 BM25 命中 = 0.5"的假象。
2. 无相关命中时返回空或明确低分，而不是给出自信的 0.5。
3. 降低 MCP 读取路径的上下文消耗（embedding、preview 长度）。
4. 收敛写入侧的重复、标签与作用域卫生问题。
5. 降低 Dream 合并造成的软删比例，保留关键数字锚点。

### 非目标

1. 不引入外部向量数据库或图数据库。
2. 不改变"原文直存"的写入哲学。
3. 不在本计划内重写 Dream 的 LLM 抽取逻辑。

## 3. 优先级总览

| 编号 | 优先级 | 主题 | 预估 | 依赖 | 验收方式 |
|---|---|---|---|---|---|
| T1 | P0 | 检索分数校准与可信门控 | 4-6h | 无 | 负例返回空 + retrieval_gate |
| T2 | P1 | MCP/CLI 读取负载优化 | 2-3h | T1 字段可复用 | 体积对比 + 单测 |
| T3 | P1 | 写入去重、标签与关系 | 4-6h | 无 | 单测 + 数据抽检 |
| T4 | P2 | Dream 保留策略与数字锚点 | 4-6h | T1（检索仍可命中） | DreamRun 审计 + 抽检 |
| T5 | P2 | 作用域与跨 project 归因 | 3-4h | T3 | 单测 + 运维工具 |
| T6 | P2 | 评测体系补强（负例/门禁） | 2h | T1 | gate 退出码 |

建议顺序：T1 → T6（同步补负例）→ T2 → T3 → T5 → T4。

## 4. 逐项计划

### T1 检索分数校准与可信门控（P0）

#### 问题

1. 融合分是相对量，单路 top 固定为 0.5。
2. `min_score` 只作用于向量原始 cos，FTS 侧无过滤；向量全被滤空后，纯 BM25 命中仍以 0.5 返回。
3. 用户与 agent 都无法从分数判断"这是语义命中还是单字词碰撞"。

#### 推荐方案（上策）

采用 RRF 排序，同时透出三路原始分，并以原始分做门控，而不是对融合分做绝对阈值。

| 组成 | 处理 |
|---|---|
| 向量分 | 保留原始余弦 `vector_score`，继续作为 `min_score` 的绝对入口阈值 |
| FTS 分 | 保留原始 BM25 `fts_score`，并计算查询词覆盖率 `coverage` |
| 排序分 | RRF：`sum(1/(k+rank_i))`，k=60；用于最终排序 |
| 融合分 | 归一化 RRF 到 0-1 作为兼容 `score`，但不用于绝对阈值 |
| 无命中门控 | 向量无候选且 FTS 覆盖率低于阈值时返回空 |
| 相对截断 | 可选开关，默认关闭；基于原始分而非 RRF 分 |

决策点：

1. `coverage` 阈值默认值（建议 0.30，且至少命中 2 个查询词）。
2. 是否保留 `hybrid_alpha` 配置；建议保留但仅用于 RRF 加权，不再产生固定 0.5。
3. `SearchHit.score` 的兼容策略：保留字段，新增原始分；旧客户端不受影响。

备选：

- 中策：绝对分校准 `score = alpha*cos + (1-alpha)*coverage`，不引入 RRF。改动更小，但排序能力弱于 RRF。
- 下策：仅让 FTS 贡献乘以覆盖率、低覆盖丢弃，并加相对截断。可作为 30 分钟止血，但分数仍不完全可解释。

#### 改动面

1. 协议：`SearchHit` 增加 `vector_score`、`fts_score`、`coverage`（serde default）。
2. 融合：`hybrid_store.rs` 重写 `merge_and_normalize`，或新增 `merge_rrf`。
3. FTS：`tantivy_store.rs` 填充 coverage（基于查询词与文档内容），或由融合层用 preview 估算。
4. 处理器：`memrecd/src/server/handler/search.rs` 透传新字段。
5. 客户端：`memrec` CLI 与 MCP schema 增加可选字段说明。
6. 配置：`SearchConfig` 增加 coverage 阈值与相对截断开关。

#### 验收

1. 负例"完全无关的烘焙面包配方"返回空，或所有结果低于 `min_score`。
2. 相关查询 top1 与 top2 分差可感知，且 top1 为语义命中。
3. `retrieval_gate.sh` 退出码 0，hit@5 与 MRR 不低于现有阈值。
4. 新增单测：单路 BM25 命中不得达到默认 `min_score`；无向量候选且低覆盖时结果为空。
5. 新增负例到评测集或门禁脚本。

#### 风险

1. 阈值调整可能影响既有调用方对 0.5 的依赖，需要在 CHANGELOG 标注语义变更。
2. RRF 分与旧分数不可直接比较，需在响应中明确 `score` 的语义。

### T2 MCP/CLI 读取负载优化（P1）

#### 问题

1. `mr_get` 返回完整 `Memory`，含 1024 维 `embedding`，单条即可消耗数千 token。
2. `mr_search` 只返回 200 字符 `content_preview`，通常还需二次 `mr_get`。
3. `mr_get` 对 `is_deleted=true` 的记录不提示，存在误用风险。

#### 方案

1. `mr_get` 增加 `include_embedding`，默认 false；服务端序列化时剔除 embedding。
   - 已完成（0.11.0）：协议响应统一改用 `MemoryView`，`Add`/`Get`/`List` 均不再返回 `embedding`，无需开关。
2. `mr_get` 默认对已删除记录返回明确状态标记（例如 `deleted: true` 且附 `deleted_at`），或提供 `allow_deleted` 参数。
3. `mr_search` 的 preview 长度可配置（默认 500，上限 2000），或增加 `snippet` 字段返回命中片段而非固定头部。
4. CLI 增加对应参数，保持默认低消耗。

#### 验收

1. 同一记忆 `mr_get` 默认响应体积相比现状下降一个数量级。
2. 搜索后无需二次 `mr_get` 即可判断内容相关性。
3. 软删记录在默认读取路径中不再被当作有效证据。

### T3 写入去重、标签与关系（P1）

#### 问题

1. 单条写入的 tags 未去重，已出现 `["dream-integrated","dream-integrated","dream-fact"]`。
2. 跨 project 的同源记忆未触发 duplicates。
3. "更正"类记忆只追加新条并给旧条打 `superseded` 标签，旧条仍参与默认检索。

#### 方案

1. tags 写入前做规范化与去重（去空白、保留大小写策略、稳定顺序）。
2. 去重检测增加范围选项（project / global / all），并把 duplicates 结果透出到 MCP 响应。
3. 引入显式 `supersedes_id`：新条记录被替代条 ID；旧条标记 superseded 并在默认检索中降权或排除，除非显式 query 包含历史。
4. 可选：为同源跨 project 记忆提供 `canonical_id` 或归并建议，不自动合并。

#### 验收

1. 重复标签不再落库。
2. 跨 project 同源写入能命中 duplicates。
3. 默认检索不再返回已被 supersede 的旧结论。
4. 数据抽检：tags 重复率、superseded 比例可统计。

### T4 Dream 保留策略与数字锚点（P2）

#### 问题

1. 当前 active 1912 / deleted 753，约 28% 处于软删状态；部分关键经验落在 deleted 集。
2. 合并摘要可能丢失数字、路径、行号、命令等不可改写的事实。
3. 原条被软删后不可检索，只能靠摘要。

#### 方案

1. 合并前用 `extract_summary_clues` 提取锚点，合并后校验锚点保留率，低于阈值则回退到保留原条。
2. 被合并原条改为 `archived` 状态（可检索、默认降权），而不是直接软删；或提供可配置的保留窗口。
3. DreamRun 审计增加：锚点保留率、软删比例、可回溯率。
4. 保留策略按类型细化（已有 preserve_types 基础上扩展）。

#### 验收

1. 软删比例目标下降（建议 < 15%），且关键类型不进入软删。
2. 锚点保留率达到设定阈值。
3. 被合并原条仍可通过检索命中（降权）。
4. Dream 审计报告可量化上述指标。

### T5 作用域与跨 project 归因（P2）

#### 问题

1. 无 `.mr_pid` 时写入静默落到 global，用户无感知。
2. 同一事件散落在多个 project_id（仓库多份拷贝导致）。
3. scope 推断依赖目录标记，跨副本不稳定。

#### 方案

1. 写入时若 scope 推断为 global 且调用方未显式指定 `is_global`，返回结构化 warning（日志与响应字段，不依赖 stdout）。
2. 提供 project 归因工具：按 git remote / 内容指纹 / 时间窗口给出候选归并，人工确认后执行。
3. 记录 project_id 的来源（marker / remote / explicit），便于审计。
4. 可选：`mr_list`/`mr_search` 支持按 project 别名聚合。

#### 验收

1. 无标记写入有明确提示。
2. 能列出一组疑似同源 project 并给出归并建议。
3. 归并后检索不再因副本分叉而漏召回。

### T6 评测体系补强（P2）

#### 问题

当前 `eval/retrieval_golden.jsonl` 只有正例，无法捕获"无关查询给出高分"的回归。

#### 方案

1. 增加负例：`relevant` 为空，期望 top1 低于阈值或结果为空。
2. `retrieval_gate.sh` 增加第三项：负例的"最高分不得超过 X"。
3. 增加分差指标：top1 与 top2 的 gap，防止头部不可区分。
4. 记录每次 gate 的 hit@5、MRR、负例最高分，形成趋势。

#### 验收

1. 负例门禁在修复前失败、修复后通过。
2. 门禁报告包含分差与负例指标。

## 5. 实施与门禁

每一项独立提交，遵循 `AGENTS.md` 的部署流程：

1. `cargo build --release` 与 `cargo test --release` 通过。
2. 停止 `memrecd`，备份 `~/.memrec`。
3. 部署 `memrec`、`memrecd` 到 `~/.local/bin` 与 `~/.cargo/bin`。
4. 启动服务，等待模型与 FTS 索引就绪。
5. 运行 `bash scripts/retrieval_gate.sh`，退出码必须为 0。
6. 确认服务状态与 `memrec stats` 一致。

T1 属于检索排序改动，门禁不可跳过；T2/T3/T5 涉及读取与写入路径，需补对应单测；T4 需在测试数据上先跑 `dry_run`。

## 6. 附：复现用例

负例查询：`完全无关的烘焙面包配方`

期望：返回空，或最高分低于默认 `min_score`；不得出现 0.5。

相关查询：`glibc arena chunk 偏移 malloc_state`

期望：top1 为语义相关记忆，且与 top2 有可感知分差。

## 7. 实施记录

### T1 检索分数校准与可信门控（完成，2026-10-26）

- 决策：采用"绝对分校准为主 + 加权 RRF 作为同分裁决"，未采用"纯 RRF 排序"。原因：RRF 名次分无法同时作为 `min_score` 的绝对阈值；校准分让阈值恢复可解释语义，同时保留 RRF 用于同分裁决与可解释性。
- 分数模型：`score = 1 - (1 - vector_cos)(1 - coverage)`。向量单路退化为原始余弦，全文单路退化为覆盖率，双路命中相互增强。
- 门控：全文单路 `coverage < 0.30` 直接丢弃；融合后要求"向量余弦 >= min_score 或覆盖率 >= 0.30"，且校准分 >= min_score。避免两路都弱的候选靠 noisy-OR 抬到阈值之上。
- 透出字段：`vector_score`、`fts_score`、`coverage`、`rank_score`（均 serde default，向后兼容）。
- 回归：新增 4 个融合单测（绝对分、低覆盖丢弃、双路 noisy-OR、无关查询空结果）与端到端无关查询用例。
- 取舍：`MIN_FTS_COVERAGE=0.30` 对强改写查询可能损失少量纯全文召回；当前金标集 hit@5 为 0.84（阈值 0.80），MRR 0.726（阈值 0.55）。

### T6 评测体系补强（完成，2026-10-26）

- 修复 3 条悬空金标目标（原目标已被软删）：`dead-import`、`kalman`、`pnl-metric` 替换为仍存活的同类记忆，替换后均命中 top1。
- `retrieval_gate.sh` 新增负例门禁：三条无关查询的 top1 校准分必须严格低于 0.5。
- 注意：原负例"完全无关的烘焙面包配方"已被本项目自身分析记忆污染（该记忆正文包含此短语），故负例改用其他无关查询；该现象也说明"查询词被自身记录命中"属于数据层面，而非分数校准问题。

### Dream 产出与保留治理（完成，2026-11-05）

- 源记忆不再软删：Dream 处理后将源记忆标记为 `dream-processed`（附 `dream_processed_at` / `dream_processed_run_id`），仅排除未来 Dream 候选，原文继续可检索。
- 产出预算：新增 `dream_out_size`（默认 30，facts + summaries 总预算）与 `max_facts_per_cluster`（默认 3）；每簇 facts 上限由预算推导并写入提炼提示词，要求合并相似事实、优先结论/数据/路径。
- 事实去重阈值默认 0.97 → 0.92；Dream 产出与已处理源不再进入后续候选，避免递归整合。
- 口径修正：`input_count`（输入）与 `produced_count`（产出）进入 DreamResult/DreamRun；`integrated_count` 保留为输入数兼容字段。
