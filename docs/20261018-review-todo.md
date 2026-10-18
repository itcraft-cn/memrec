# memrec 双点评合并解析与 TODO（2026-10-18）

来源：两份独立点评（点评A：运行时/daemon 视角；点评B：数据/生命周期视角）。
本文档已将全部论据与代码核对（grep 定位），并与 2026-10-18 图扩散量化结论
（docs/design/dream-graph-quantification.md 及 memrec 记忆 0bc12f6a/0d9a2909）交叉印证。
不在文中展开点评全文，只保留经核验的结论与行动项。

## 1. 点评可信度判断

- 两份点评问题清单重叠度极低（仅全量 list 重合），互补盲区扫描，可信度高。
- 关键论据逐条核验结果：
  - 全量 `list(usize::MAX)`：实存 5 处（memrecd/src/daemon/mod.rs 282、334；
    memrecd/src/server/handler.rs 651；mr-ability/src/dream/processor.rs 248、571）。
  - MMR 用 200 字符 content_preview：属实（mr-ability/src/storage/hybrid_store.rs 148
    `text()`；上游写入统一 `content.chars().take(200)`）。
  - Dream 产出 `project_id = Some(Uuid::nil())`：属实（processor.rs 310，注释自述"与
    历史 Dream 产出保持一致"），与 `project_id = None` 的普通全局记忆并存。
  - `list_by_tag("dream-integrated", 1000)` 硬编码截断：属实（processor.rs 1133），
    既是性能问题也是正确性问题（超 1000 条后幂等补删漏判）。
  - 死接口 ProjectStorage / ConfigStorage：属实（mr-ability/src/storage/traits.rs 55、66，
    仅 trait 定义无实现）。
  - mark_applied 读改写无事务：属实（propagation_store.rs 160），当前有 DreamLock 与
    幂等短路保护，实际风险低。
  - MCP call_daemon 字符串方法名手工映射：属实（memrec/src/mcp/mod.rs 550 起）。
- 与既有结论冲突修正：点评A 将"quota 保留名额使图扩散生效"列为主要亮点，但量化实验
  结论是图扩散开/关在 hit@k / recall@k / MRR / nDCG 上一致（k=20 仅 +0.003），
  即图扩散当前实质无效。此项保留为待办而非已完成。

## 2. 已确认亮点（无需行动，记录以防止退化）

- IndexingStorage 装饰器：Dream 写路径统一镜像到混合检索索引（commit 760d1b5），
  一类 bug 从靠纪律变为靠类型。
- Dream 两段式 + source_ids 幂等；失败不写 state（可重试语义明确）。
- 上游融合去重（阈值 0.97）；语义边默认灰度关闭、失败不中断。
- dry-run 全流程跑通后在写入处判断，保证 dry-run 与真实执行一致。
- 启动期自愈：FTS 全量重建、缺失嵌入补生成、删除同步摘索引。
- 评测框架（hit@k / recall@k / MRR / nDCG）与运行期 DreamRun.metrics 互补。
- LLM 定位为增强项而非依赖项，外部依赖故障不瘫痪主流程。

## 3. TODO

### P0 性能天花板（随数据量单调恶化）

- [ ] T1 全量 list 去除：为 MemoryStorage 增加"按 created_at 区间 / 标签 / 重要性
  范围"的索引下推查询（RocksDB 已有 importance 列族，扩展时间戳列族）。
  覆盖全部 5 处调用点；启动期 reconcile/rebuild 的全量扫描可保留（自愈场景合理），
  但 handle_list 与 Dream 候选必须走下推。
  - 验收：十万条量级时 handle_list 与 Dream 候选阶段不再全量反序列化。
- [ ] T2 collect_integrated_source_ids 重构：从"扫 dream-integrated 标签前 1000 条"
  改为按 dream_run_id 反查（或在整合记忆 metadata 维护反向索引），消除截断漏判。
  - 验收：补删测试在超过 1000 条已整合源场景下仍幂等正确。

### P1 语义正确性

- [ ] T3 MMR 相似度输入：SearchHit 在 rerank 阶段携带全文 token（或全文哈希），
  content_preview 仅用于展示；若确认保留 preview 粗排，须在评分链文档中明确标注。
  - 验收：构造"开头相同、结论相反"两条记忆，MMR 判定不再高度相似。
- [ ] T4 全局语义统一：将"全局"收敛到 scope（MemoryScope::Global / Project /
  Workspace），project_id 仅表归属；Dream 产出迁移为 project_id = None +
  scope = Global，附带历史数据迁移（现网 Some(nil) 记录改写）。
  过渡期内禁止新代码以 `project_id.is_none()` 或 `Some(nil)` 单独判断全局。
  - 验收：list_by_project(nil) 调用点归零；个人摘要、rocksdb_vector 过滤、
    CLI Scope::Global 三处行为一致。
- [ ] T5 错误链保留：DreamError（及同模式错误）改 thiserror #[from] 保留原始类型，
  日志/审计以 {:#} 输出完整错误链；String 化仅允许在协议边界。

### P2 可维护性

- [ ] T6 handle_add 拆分：去重决策、更新窗口合并、写入与索引同步拆分为独立方法或
  AddHandler；目标单方法职责单一、长度减半。
- [ ] T7 MCP 方法名双轨收敛：call_daemon 接收 RequestAction 枚举统一走 serde，
  Response 按枚举 match，消除新增接口改两处的漂移。
- [ ] T8 死接口处理：ProjectStorage / ConfigStorage 要么实现要么删除删除预期更大，
  若为规划预留则补注释说明。
- [ ] T9 mark_applied 事务化：PropagationEngine 若未来并发调用，改 RocksDB
  WriteBatch 单次提交；当前 DreamLock 保护下降级为低优先观察项。

### P3 改进项

- [ ] T10 图扩散再验证与并入策略：按既有待办将图命中并入改 RRF 或保留名额，
  在 LlmEdgeGenerator 候选池 top-k 限幅修复后重新跑评测（量化结论其当前无效）。
- [ ] T11 读路径容错收紧：handle_list / stats 的 unwrap_or_default 区分
  "业务无数据"与"存储故障"（前者返回空，后者报错或至少 warn 告警）。
- [ ] T12 图扩散并行化：与混合搜索 tokio::join! 或加超时退化为纯向量检索。
- [ ] T13 TantivyStore::count() 语义标注（num_docs 含未 compact 文档），
  如做监控指标需 reload 后取值。
- [ ] T14 测试 TempDir 模式收敛：std::mem::forget(dir) 改为固定路径 tempdir_in
  或 Arc<TempDir> 由 Router 持有。
- [ ] T15 配置热加载：SIGHUP 重载或 ReloadConfig RPC，至少覆盖阈值类配置
  （duplicate_threshold / update_threshold）。

## 4. 执行顺序建议

T1 -> T2（同一次存储层改造内完成）；T3 -> T4（先修精度语义再动全局语义，
T4 含数据迁移需单独发布）；T5、T6、T7、T8 并行小步提交；T10 在 T1-T4 稳定后执行。
每项完成后按 AGENTS.md 验收流程：编译 -> 停服 -> 备份 ~/.memrec -> 部署双目录 ->
测试 -> 还原数据。
