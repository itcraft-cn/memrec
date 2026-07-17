# LLM 生成检索规则设计

## 参考来源

- **论文**: MemRec: Collaborative Memory-Augmented Agentic Recommender System
- **论文 URL**: https://arxiv.org/abs/2601.08816
- **论文解读**: https://papernotes.org/ACL2026/recommender/memrec_collaborative_memory-augmented_agentic_recommender_system/
- **GitHub**: https://github.com/rutgerswiselab/memrec

## 背景与动机

### 当前问题

我们的 MemRec 检索仅依赖向量相似度 + BM25，缺乏可解释的规则过滤：

1. **不可解释**: 向量检索是黑盒，无法解释为什么返回某条记忆
2. **无法定制**: 用户无法定义"什么条件下应该检索什么"
3. **效率瓶颈**: 大量无关记忆参与排序，浪费计算

### 论文启发

论文 MemRec 提出 Curate-then-Synthesize：
1. **Curate 阶段**: LLM 生成检索规则（如"如果查询关于性能优化，优先返回 CausedBy 边的记忆"）
2. **Synthesize 阶段**: 用规则过滤候选，再合成最终结果

规则由 LLM 离线生成，在线毫秒级应用。

## 设计目标

1. LLM 离线生成检索规则，在线毫秒级应用
2. 规则可解释、可调整、可禁用
3. 规则覆盖常见检索场景
4. 规则自动更新，适应用户习惯变化

## 概念定义

### RetrievalRule

| 属性 | 类型 | 说明 |
|------|------|------|
| id | Uuid | 规则 ID |
| name | String | 规则名称 |
| description | String | 规则描述 |
| condition | RuleCondition | 触发条件 |
| action | RuleAction | 执行动作 |
| priority | i32 | 优先级，越大越先执行 |
| enabled | bool | 是否启用 |
| hit_count | u32 | 命中次数 |
| created_at | DateTime | 创建时间 |
| updated_at | DateTime | 更新时间 |
| auto_generated | bool | 是否自动生成 |

### RuleCondition

| 属性 | 类型 | 说明 |
|------|------|------|
| query_pattern | String | 查询模式（正则或关键词） |
| memory_type | Option<Vec<MemoryType>> | 限定记忆类型 |
| project | Option<String> | 限定项目 |
| tags | Option<Vec<String>> | 限定标签 |
| time_range | Option<TimeRange> | 限定时间范围 |

### RuleAction

| 属性 | 类型 | 说明 |
|------|------|------|
| boost_type | BoostType | 动作类型 |
| factor | f32 | 影响因子 |
| edge_types | Option<Vec<EdgeType>> | 沿哪些边扩散 |
| max_depth | Option<u32> | 扩散深度 |

### BoostType

| 类型 | 说明 |
|------|------|
| Boost | 提升得分（factor > 1.0） |
| Suppress | 抑制得分（factor < 1.0） |
| Filter | 完全过滤（factor = 0.0） |
| Expand | 沿图扩散检索 |

### 规则示例

```json
{
  "id": "rule-001",
  "name": "性能查询优先因果链",
  "description": "当查询涉及性能优化时，优先沿 CausedBy 边扩散",
  "condition": {
    "query_pattern": "性能|优化|瓶颈|latency|performance",
    "memory_type": ["knowledge", "decision"]
  },
  "action": {
    "boost_type": "Expand",
    "factor": 1.5,
    "edge_types": ["CausedBy", "References"],
    "max_depth": 2
  },
  "priority": 100,
  "enabled": true,
  "auto_generated": true
}
```

## 架构设计

### 整体架构

```
┌──────────────────────────────────────────────────────┐
│              Rule-Based Retrieval                      │
├──────────────────────────────────────────────────────┤
│                                                       │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐        │
│  │  Rule     │   │  Rule     │   │  Rule     │        │
│  │ Generator │   │  Store    │   │  Engine   │        │
│  │ (离线LLM) │──▶│           │──▶│ (在线ms级) │        │
│  └──────────┘   └──────────┘   └──────────┘        │
│       │                              │               │
│       ▼                              ▼               │
│  定期分析用户习惯              规则匹配+动作执行      │
│  生成/更新规则                 修改检索结果           │
│                                                       │
└──────────────────────────────────────────────────────┘
```

### 检索流程（含规则）

```
┌─────────────┐
│ 用户查询     │
└──────┬──────┘
       │
       ▼
┌─────────────────────────┐
│ Step 1: 基础向量检索     │
│ 获取 top-k 候选记忆     │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ Step 2: 规则匹配        │
│ 遍历启用规则            │
│ 匹配 condition          │
│ 按 priority 排序        │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ Step 3: 规则执行        │
│ - Boost: 提升得分       │
│ - Suppress: 降低得分    │
│ - Filter: 移除          │
│ - Expand: 图扩散补充    │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ Step 4: 重排序+返回     │
└─────────────────────────┘
```

## 流程设计

### 规则生成流程

**触发条件**: 定期（如每周）或手动触发

```
┌─────────────────────────┐
│ 收集用户检索历史         │
│ - 查询模式统计           │
│ - 点击/采纳统计          │
│ - 记忆类型分布           │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ LLM 分析检索模式         │
│ 识别可优化的检索场景     │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 生成候选规则             │
│ - condition 匹配模式     │
│ - action 优化策略        │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 验证规则有效性           │
│ 在历史数据上回测         │
│ 保留有效规则             │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 存储规则，默认启用       │
└─────────────────────────┘
```

**LLM Prompt 设计**:

```
你是一个检索优化专家。基于以下用户检索历史，生成检索优化规则。

检索历史统计：
- 高频查询模式：{query_patterns}
- 常检索记忆类型：{memory_types}
- 高采纳率场景：{high_adoption_scenarios}
- 低采纳率场景：{low_adoption_scenarios}

请生成 3-5 条检索规则，每条规则包含：
1. name: 规则名称
2. description: 规则描述
3. condition: 触发条件（query_pattern, memory_type, tags）
4. action: 执行动作（Boost/Suppress/Filter/Expand + factor）
5. priority: 优先级（1-100）

输出 JSON 数组格式。
```

### 规则匹配流程

```
┌─────────────────┐
│ 输入查询 + 候选  │
└──────┬──────────┘
       │
       ▼
┌─────────────────────────────┐
│ 遍历启用规则（按 priority）  │
│                               │
│ for rule in enabled_rules:   │
│   if matches(query, rule):  │
│     apply(rule, candidates) │
└──────┬──────────────────────┘
       │
       ▼
┌─────────────────────────────┐
│ 返回修改后的候选列表         │
└─────────────────────────────┘
```

**匹配逻辑**:

| 条件字段 | 匹配方式 |
|----------|----------|
| query_pattern | 正则匹配查询文本 |
| memory_type | 候选记忆类型在列表中 |
| project | 候选记忆项目匹配 |
| tags | 候选记忆标签有交集 |
| time_range | 候选记忆在时间范围内 |

### 规则更新流程

```
┌─────────────────────────┐
│ 定期评估规则效果         │
│ - hit_count 统计         │
│ - 采纳率统计             │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 低效规则处理             │
│ - hit_count=0 且>30天:  │
│   自动禁用               │
│ - 采纳率<20% 且>7天:    │
│   降低 priority          │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 重新生成规则             │
│ 基于最新检索历史         │
└─────────────────────────┘
```

## 存储设计

### RocksDB 列族设计

| Column Family | Key | Value | 说明 |
|---------------|-----|-------|------|
| rules | rule_id | RetrievalRule | 规则主表 |
| rules_by_priority | priority | Vec<rule_id> | 按优先级索引 |
| rule_stats | rule_id | RuleStats | 规则统计 |

### RuleStats

| 属性 | 类型 | 说明 |
|------|------|------|
| rule_id | Uuid | 规则 ID |
| hit_count | u32 | 命中次数 |
| adoption_count | u32 | 采纳次数 |
| last_hit | DateTime | 最后命中时间 |
| last_adoption | DateTime | 最后采纳时间 |

## 接口设计

### 生成规则

```
POST /api/rules/generate

Request:
{
  "max_rules": 5,
  "analysis_window_days": 30
}

Response:
{
  "generated_rules": [
    {
      "rule_id": "uuid-xxx",
      "name": "性能查询优先因果链",
      "status": "enabled"
    }
  ]
}
```

### 列出规则

```
GET /api/rules?enabled_only=true

Response:
{
  "rules": [
    {
      "rule_id": "uuid-xxx",
      "name": "性能查询优先因果链",
      "description": "...",
      "priority": 100,
      "enabled": true,
      "hit_count": 42,
      "auto_generated": true
    }
  ]
}
```

### 更新规则

```
PUT /api/rules/{rule_id}

Request:
{
  "enabled": false,
  "priority": 50
}

Response:
{
  "rule_id": "uuid-xxx",
  "status": "updated"
}
```

### 删除规则

```
DELETE /api/rules/{rule_id}

Response:
{
  "rule_id": "uuid-xxx",
  "status": "deleted"
}
```

## 性能考量

### 规则匹配延迟

- 规则数量通常 < 50
- 每条规则匹配 < 0.1ms
- 总匹配延迟 < 5ms

### 规则生成成本

- 每次生成调用 1 次 LLM
- Input: ~1000 tokens, Output: ~500 tokens
- 每周 1 次，成本可忽略

## 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| 规则冲突 | 按 priority 排序，高优先级先执行 |
| 规则过时 | 定期评估，自动禁用低效规则 |
| 规则偏见 | 保留基础检索结果，规则仅调整排序 |
| 生成质量低 | 回测验证，低效规则不启用 |

## 验收标准

1. RetrievalRule 结构已定义，存储层已支持
2. LLM 可自动生成检索规则
3. 规则在线匹配延迟 < 5ms
4. 4 种 BoostType 已实现
5. 规则 CRUD 接口可用
6. 规则自动评估和淘汰机制可用

## 参考资料

- MemRec 论文: https://arxiv.org/abs/2601.08816
- 论文 Section 2.1.2: Curate-then-Synthesize
- 论文 Figure 3: Curation Prompt
- 论文 Section 4.3: Ablation Study on Rules
