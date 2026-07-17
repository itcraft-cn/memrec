# Dream 增量传播设计

## 参考来源

- **论文**: MemRec: Collaborative Memory-Augmented Agentic Recommender System
- **论文 URL**: https://arxiv.org/abs/2601.08816
- **论文解读**: https://papernotes.org/ACL2026/recommender/memrec_collaborative_memory-augmented_agentic_recommender_system/
- **GitHub**: https://github.com/rutgerswiselab/memrec

## 背景与动机

### 当前问题

我们的 MemRec Dream 功能仅压缩自身记忆，不更新相关记忆：

1. **孤立压缩**: Dream 压缩后，相关记忆不知道变化
2. **信息断层**: 压缩产生的新知识无法传播到邻居
3. **一致性缺失**: 关联记忆之间可能产生矛盾

### 论文启发

论文 MemRec 提出异步批量传播（Asynchronous Batch Propagation）：
- 当用户记忆更新时，增量传播到 k 个邻居
- 传播内容经过 Information Bottleneck 压缩（≤1800 tokens）
- O(1) 复杂度：每个节点只传播给固定数量邻居

我们借鉴此思想，在 Dream 整合时增量更新记忆图中的邻居。

## 设计目标

1. Dream 整合时不仅压缩自身，还增量更新邻居
2. 传播内容精炼，避免信息过载
3. 异步执行，不阻塞主流程
4. 幂等设计，重复传播不产生副作用

## 概念定义

### PropagationDelta

传播增量，描述一次传播的内容。

| 属性 | 类型 | 说明 |
|------|------|------|
| id | Uuid | 传播 ID |
| source_id | Uuid | 源记忆 ID（被整合的记忆） |
| target_id | Uuid | 目标记忆 ID（邻居记忆） |
| delta_type | DeltaType | 增量类型 |
| content | String | 增量内容 |
| token_count | u32 | Token 数量 |
| created_at | DateTime | 创建时间 |
| applied | bool | 是否已应用 |

### DeltaType

| 类型 | 说明 | 示例 |
|------|------|------|
| KnowledgeUpdate | 知识更新 | "Tokio 版本从 1.x 升级到 2.x" |
| Contradiction | 矛盾发现 | "之前偏好 SQLite，现在改为 PostgreSQL" |
| Refinement | 精炼补充 | "补充：零拷贝方案需注意对齐问题" |
| Obsolescence | 过时标记 | "v1 设计已废弃，使用 v2" |

### PropagationConfig

| 参数 | 默认值 | 说明 |
|------|--------|------|
| max_neighbors | 8 | 最大传播邻居数 |
| max_delta_tokens | 200 | 单次传播最大 token 数 |
| propagation_depth | 1 | 传播深度（仅直接邻居） |
| batch_size | 10 | 批量传播大小 |
| retry_limit | 3 | 失败重试次数 |

## 架构设计

### 整体架构

```
┌──────────────────────────────────────────────────────┐
│                  Dream Propagation                    │
├──────────────────────────────────────────────────────┤
│                                                       │
│  ┌─────────┐    ┌──────────────┐    ┌─────────┐     │
│  │  Dream   │───▶│  Propagation │───▶│  Apply   │     │
│  │  Engine  │    │  Queue       │    │  Engine  │     │
│  └─────────┘    └──────────────┘    └─────────┘     │
│       │                │                  │          │
│       ▼                ▼                  ▼          │
│  压缩+生成增量    异步队列调度      更新邻居记忆      │
│                                                       │
└──────────────────────────────────────────────────────┘
```

### 传播流程

```
Dream 整合完成
       │
       ▼
┌─────────────────────────┐
│ 查找记忆图邻居           │
│ 按边权重排序             │
│ 取 top-k 邻居            │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 为每个邻居生成增量       │
│ LLM 分析：              │
│   整合后记忆 + 邻居记忆  │
│   → 生成 PropagationDelta│
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 压缩增量内容             │
│ Token 预算控制           │
│ ≤ max_delta_tokens      │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 推入传播队列             │
│ 异步执行                 │
└─────────────────────────┘
```

## 流程设计

### 增量生成流程

**触发条件**: Dream 整合完成后

```
┌─────────────────────────┐
│ Dream 整合完成           │
│ 旧记忆 → 新记忆         │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 从记忆图获取邻居列表     │
│ 按边权重降序排列         │
│ 取 top-k (默认 8)       │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────────────────┐
│ 对每个邻居：                         │
│   LLM 分析整合后记忆与邻居的关系     │
│   生成增量内容                       │
│   判断增量类型                       │
│   压缩到 token 预算内                │
└──────┬──────────────────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 批量推入传播队列         │
└─────────────────────────┘
```

**LLM Prompt 设计**:

```
你是一个记忆传播分析专家。

整合后的记忆：
{consolidated_memory}

邻居记忆：
{neighbor_memory}

两者关系：{edge_type} (权重: {weight})

请分析整合后的变化对邻居记忆的影响，生成增量更新。

输出 JSON：
{
  "delta_type": "KnowledgeUpdate|Contradiction|Refinement|Obsolescence|None",
  "content": "简洁的增量描述，不超过200 tokens",
  "confidence": 0.85
}
```

### 增量应用流程

**执行方式**: 异步，从传播队列消费

```
┌─────────────────────────┐
│ 从传播队列取增量         │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 检查幂等性               │
│ 同一 source→target      │
│ 只保留最新增量           │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 应用增量到目标记忆       │
│ - KnowledgeUpdate:      │
│   追加到记忆内容         │
│ - Contradiction:        │
│   标记矛盾，降低置信度   │
│ - Refinement:           │
│   补充细节               │
│ - Obsolescence:         │
│   标记过时               │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 更新目标记忆的 Facet     │
│ 更新边权重               │
│ 标记增量已应用           │
└─────────────────────────┘
```

### 传播队列设计

```
┌─────────────────────────────────────────┐
│           Propagation Queue              │
├─────────────────────────────────────────┤
│  Priority:                               │
│  1. Contradiction (最高优先级)           │
│  2. Obsolescence                         │
│  3. KnowledgeUpdate                      │
│  4. Refinement (最低优先级)              │
│                                          │
│  Concurrency: 4 workers                  │
│  Batch: 10 items per batch               │
│  Retry: 3 times with backoff             │
└─────────────────────────────────────────┘
```

### Information Bottleneck

借鉴论文的窄通道设计，控制传播信息量：

```
┌──────────────────────────────────────┐
│       Information Bottleneck          │
├──────────────────────────────────────┤
│                                       │
│  整合后记忆 (可能很长)                │
│       │                               │
│       ▼                               │
│  LLM 提取关键变化                     │
│       │                               │
│       ▼                               │
│  压缩到 ≤ 200 tokens                  │
│       │                               │
│       ▼                               │
│  传播增量                             │
│                                       │
└──────────────────────────────────────┘
```

**Token 预算分配**:

| 用途 | Token 预算 |
|------|-----------|
| delta_type 描述 | 10 |
| 增量内容 | 150 |
| confidence 说明 | 20 |
| 保留 | 20 |
| **总计** | **200** |

## 存储设计

### RocksDB 列族设计

| Column Family | Key | Value | 说明 |
|---------------|-----|-------|------|
| propagation_deltas | delta_id | PropagationDelta | 增量主表 |
| pending_propagations | (target_id, priority) | Vec<delta_id> | 待应用增量队列 |
| applied_propagations | (source_id, target_id) | delta_id | 已应用增量索引（幂等检查） |

### 队列持久化

传播队列持久化到 RocksDB，确保重启后不丢失：
- pending_propagations: 按优先级排序的待处理增量
- 进程启动时扫描 pending 队列，恢复未完成的传播

## 接口设计

### 触发传播

```
POST /api/memories/{memory_id}/propagate

Request:
{
  "max_neighbors": 8,
  "max_delta_tokens": 200
}

Response:
{
  "propagation_id": "uuid-xxx",
  "neighbor_count": 5,
  "status": "queued"
}
```

### 查询传播状态

```
GET /api/memories/{memory_id}/propagations

Response:
{
  "memory_id": "uuid-xxx",
  "propagations": [
    {
      "delta_id": "uuid-yyy",
      "target_id": "uuid-zzz",
      "delta_type": "KnowledgeUpdate",
      "status": "applied",
      "created_at": "2026-07-17T10:00:00Z"
    }
  ]
}
```

### 手动应用增量

```
POST /api/propagations/{delta_id}/apply

Response:
{
  "delta_id": "uuid-yyy",
  "status": "applied"
}
```

## 性能考量

### 传播延迟

- 邻居查找：< 5ms
- LLM 增量生成：1-3s/邻居
- 批量处理 8 邻居：2-5s
- 异步执行，不阻塞主流程

### 存储开销

- 每个增量约 300-500 字节
- 每次 Dream 产生 5-8 个增量
- 1000 次 Dream 约 2-4 MB 增量数据

### LLM 调用成本

- 每个邻居 1 次 LLM 调用
- Input: ~600 tokens, Output: ~200 tokens
- 批量处理可优化

## 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| 传播雪崩 | 限制传播深度为 1，不级联传播 |
| 增量质量低 | LLM 生成后验证，低置信度增量不应用 |
| 队列积压 | 批量处理 + 并发 workers |
| 幂等问题 | source→target 唯一索引，只保留最新增量 |
| 一致性延迟 | 异步设计接受短暂不一致，最终一致 |

## 验收标准

1. PropagationDelta 结构已定义，存储层已支持
2. Dream 整合后自动触发传播
3. 4 种增量类型已实现
4. 传播队列异步执行，不阻塞主流程
5. 幂等设计，重复传播无副作用
6. Information Bottleneck 控制传播信息量 ≤ 200 tokens
7. 传播延迟 < 5s（8 邻居）

## 参考资料

- MemRec 论文: https://arxiv.org/abs/2601.08816
- 论文 Section 3.2: Asynchronous Batch Propagation
- 论文 Figure 5: Propagation Pipeline
- 论文 Section 3.3: Information Bottleneck
- 论文 Table 8: Token I/O Analysis
