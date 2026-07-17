# MemRec 增强计划

## 参考来源

- **论文**: MemRec: Collaborative Memory-Augmented Agentic Recommender System
- **论文 URL**: https://arxiv.org/abs/2601.08816
- **论文解读**: https://papernotes.org/ACL2026/recommender/memrec_collaborative_memory-augmented_agentic_recommender_system/
- **GitHub**: https://github.com/rutgerswiselab/memrec

## 背景

论文 MemRec 提出了协同记忆增强的推荐系统，核心创新包括：
1. 双 LLM 解耦架构（LM_Mem 记忆管理 + LLM_Rec 推理）
2. 协同记忆图（用户/物品节点通过边连接）
3. Curate-then-Synthesize（LLM 生成规则筛选 → 蒸馏成 facets）
4. 异步批量传播（O(1) 复杂度更新邻居）
5. Facet 结构化记忆（theme + confidence + evidence）

这些设计可借鉴到我们的 MemRec（AI 记忆持久化系统）。

## 目标

将论文中的优秀设计引入我们的 MemRec，提升：
- 检索精度（通过 Facet 结构化）
- 关联发现（通过记忆图边关系）
- 知识传播（通过 Dream 增量传播）
- 检索效率（通过 LLM 生成规则）

## 任务列表

### Phase 1: Facet 结构化记忆（P0）

**设计文档**: `specs/2026-07-17-facet-design.md`

**目标**:
- 引入 MemoryFacet 结构：`{theme, confidence, evidence}`
- 记忆可包含多个 facets，每个 facet 描述一个主题
- 检索时按 facet 主题匹配，而非纯向量相似度

**输出**:
- Facet 数据结构设计
- Facet 生成流程设计
- Facet 检索流程设计
- 存储层变更设计

**预计工作量**: 2 天

---

### Phase 2: 记忆图边关系（P0）

**设计文档**: `specs/2026-07-17-memory-graph-design.md`

**目标**:
- 引入记忆间关联图
- 边类型：Similar、CausedBy、References、SameProject、SameSession
- 支持沿图扩散检索

**输出**:
- 边类型定义
- 边权重计算设计
- 图存储设计
- 图遍历检索设计

**预计工作量**: 2 天

---

### Phase 3: Dream 增量传播（P1）

**设计文档**: `specs/2026-07-17-dream-propagation-design.md`

**目标**:
- Dream 整合时不仅压缩自身，还增量更新相关记忆
- 基于记忆图找到邻居，生成增量更新
- 异步执行，不阻塞主流程

**输出**:
- 传播触发条件设计
- 邻居选择策略设计
- 增量生成设计
- 异步执行设计

**预计工作量**: 3 天

---

### Phase 4: LLM 生成检索规则（P2）

**设计文档**: `specs/2026-07-17-llm-rules-design.md`

**目标**:
- 离线让 LLM 生成检索规则
- 在线用规则毫秒级过滤
- 规则可解释、可调整

**输出**:
- 规则结构设计
- 规则生成流程设计
- 规则应用流程设计
- 规则管理设计

**预计工作量**: 1 天

---

### Phase 5: Tiered Representation（P2）

**设计文档**: `specs/2026-07-17-tiered-representation-design.md`

**目标**:
- 检索结果按距离分层呈现
- 近距离：完整内容
- 中距离：截断内容
- 远距离：摘要或关键词代理

**输出**:
- 分层策略设计
- 表示形式设计
- Token 预算控制设计

**预计工作量**: 1 天

---

## 时间线

| 阶段 | 任务 | 开始日期 | 结束日期 |
|------|------|----------|----------|
| 1 | Facet 结构化记忆 | Day 1 | Day 2 |
| 2 | 记忆图边关系 | Day 3 | Day 4 |
| 3 | Dream 增量传播 | Day 5 | Day 7 |
| 4 | LLM 生成检索规则 | Day 8 | Day 8 |
| 5 | Tiered Representation | Day 9 | Day 9 |

**总工作量**: 9 天

## 风险

1. **Facet 生成质量**: 需要设计好 LLM prompt，确保生成的 facet 准确、有意义
2. **图遍历性能**: 记忆量大时，图遍历可能成为瓶颈，需设计索引
3. **传播一致性**: 异步传播可能导致短暂不一致，需设计幂等机制
4. **规则冲突**: 多条规则可能冲突，需设计优先级机制

## 验收标准

1. Facet 结构已定义，存储层已支持
2. 记忆图边关系已实现，图遍历检索可用
3. Dream 传播已实现，邻居记忆可增量更新
4. LLM 规则已生成并应用，检索效率提升
5. Tiered Representation 已实现，Token 预算可控

## 参考设计文档

- Phase 1 设计: `specs/2026-07-17-facet-design.md`
- Phase 2 设计: `specs/2026-07-17-memory-graph-design.md`
- Phase 3 设计: `specs/2026-07-17-dream-propagation-design.md`
- Phase 4 设计: `specs/2026-07-17-llm-rules-design.md`
- Phase 5 设计: `specs/2026-07-17-tiered-representation-design.md`