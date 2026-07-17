# Tiered Representation 分层呈现设计

## 参考来源

- **论文**: MemRec: Collaborative Memory-Augmented Agentic Recommender System
- **论文 URL**: https://arxiv.org/abs/2601.08816
- **论文解读**: https://papernotes.org/ACL2026/recommender/memrec_collaborative_memory-augmented_agentic_recommender_system/
- **GitHub**: https://github.com/rutgerswiselab/memrec

## 背景与动机

### 当前问题

我们的 MemRec 检索结果统一返回完整内容，存在以下问题：

1. **Token 浪费**: 远距离低相关记忆返回完整内容，浪费 LLM 上下文窗口
2. **信息过载**: 大量结果涌入，关键信息被淹没
3. **缺乏层次**: 无法区分核心/相关/边缘信息

### 论文启发

论文 MemRec 提出 Tiered Representation，根据距离分层呈现：
- **Full**: 完整内容（近距离，高相关）
- **Truncated**: 截断内容（中距离，中等相关）
- **Summary**: 摘要（远距离，低相关）
- **DenseProxy**: 仅关键词/向量代理（极远，极低相关）

## 设计目标

1. 检索结果按相关度分层呈现
2. Token 预算可控，避免上下文窗口溢出
3. 近距离信息完整，远距离信息精炼
4. 分层策略可配置

## 概念定义

### RepresentationTier

| 层级 | 说明 | Token 预算 | 适用场景 |
|------|------|-----------|----------|
| Full | 完整内容 | 不限 | score ≥ 0.8 |
| Truncated | 截断内容 | ≤ 200 tokens | 0.5 ≤ score < 0.8 |
| Summary | LLM 摘要 | ≤ 80 tokens | 0.3 ≤ score < 0.5 |
| DenseProxy | 关键词代理 | ≤ 30 tokens | score < 0.3 |

### TieredMemory

| 属性 | 类型 | 说明 |
|------|------|------|
| memory_id | Uuid | 记忆 ID |
| tier | RepresentationTier | 呈现层级 |
| content | String | 呈现内容（按层级截断/摘要/关键词） |
| score | f32 | 检索得分 |
| facet_themes | Vec<String> | 相关 Facet 主题 |
| edge_hints | Vec<EdgeHint> | 关联边提示 |

### EdgeHint

| 属性 | 类型 | 说明 |
|------|------|------|
| edge_type | EdgeType | 边类型 |
| target_summary | String | 目标记忆简要描述 |
| weight | f32 | 边权重 |

### 分层示例

```
查询: "异步编程方案"

Tier 1 (Full, score=0.92):
  "项目使用 Tokio 1.x 作为异步运行时，偏好使用 spawn 而非 spawn_blocking，
   对于 IO 密集型任务使用 tokio::io，CPU 密集型使用 rayon。"

Tier 2 (Truncated, score=0.65):
  "偏好 async-std 作为轻量级替代方案，在嵌入式场景下使用...
   [截断，共 200 tokens]"

Tier 3 (Summary, score=0.42):
  "讨论了 Rust 异步编程的性能优化策略，包括零拷贝和 pin 机制。"

Tier 4 (DenseProxy, score=0.18):
  "[async, runtime, performance, pin]"
```

## 架构设计

### 整体架构

```
┌──────────────────────────────────────────────────────┐
│            Tiered Representation Engine               │
├──────────────────────────────────────────────────────┤
│                                                       │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐        │
│  │  Scorer   │   │  Tier     │   │  Budget   │        │
│  │           │──▶│  Assigner │──▶│  Controller│       │
│  └──────────┘   └──────────┘   └──────────┘        │
│       │                              │               │
│       ▼                              ▼               │
│  计算相关度得分              控制 Token 总预算        │
│  决定层级                    降级超预算结果           │
│                                                       │
└──────────────────────────────────────────────────────┘
```

### 分层决策流程

```
┌─────────────────┐
│ 检索结果 + 得分  │
└──────┬──────────┘
       │
       ▼
┌─────────────────────────────┐
│ 按 score 排序               │
│ 计算初始 tier 分配          │
│ - score ≥ 0.8 → Full       │
│ - 0.5 ≤ score < 0.8 → Truncated │
│ - 0.3 ≤ score < 0.5 → Summary   │
│ - score < 0.3 → DenseProxy      │
└──────┬──────────────────────┘
       │
       ▼
┌─────────────────────────────┐
│ Token 预算检查               │
│ 如果总 token 超预算：       │
│ - 从低 tier 开始降级        │
│ - DenseProxy → 移除         │
│ - Summary → DenseProxy      │
│ - Truncated → Summary       │
│ - Full → Truncated          │
└──────┬──────────────────────┘
       │
       ▼
┌─────────────────────────────┐
│ 生成各层内容                 │
│ - Full: 原始内容             │
│ - Truncated: 截断到 200 tok │
│ - Summary: LLM 摘要 80 tok │
│ - DenseProxy: 关键词 30 tok │
└──────┬──────────────────────┘
       │
       ▼
┌─────────────────────────────┐
│ 组装 TieredMemory 列表      │
│ 返回                         │
└─────────────────────────────┘
```

## 流程设计

### 内容截断策略（Truncated）

```
┌─────────────────────────┐
│ 原始内容                 │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 按 sentence 边界截断     │
│ 保留完整句子             │
│ 不超过 200 tokens        │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 追加 "...[截断]" 标记    │
└─────────────────────────┘
```

### 摘要生成策略（Summary）

**方式 1: 预计算摘要（推荐）**

在添加记忆时预计算摘要，检索时直接使用：

```
添加记忆时：
┌─────────────┐     ┌──────────────┐
│ 完整内容     │────▶│ LLM 生成摘要 │
└─────────────┘     │ ≤ 80 tokens  │
                    └──────────────┘
```

**方式 2: 实时生成摘要**

检索时按需生成，延迟较高但更精准：

```
检索时：
┌─────────────┐     ┌──────────────┐
│ 完整内容     │────▶│ LLM 生成摘要 │
└─────────────┘     │ ≤ 80 tokens  │
                    └──────────────┘
```

**推荐预计算**，避免检索时额外 LLM 调用。

### DenseProxy 生成策略

```
┌─────────────────────────┐
│ 从记忆提取关键词         │
│ - tags 字段              │
│ - Facet keywords         │
│ - 内容高频词             │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 取 top-5 关键词          │
│ 格式: [kw1, kw2, ...]   │
│ ≤ 30 tokens              │
└─────────────────────────┘
```

### Token 预算控制

```
┌─────────────────────────────────────┐
│          Budget Controller           │
├─────────────────────────────────────┤
│                                      │
│  总预算: max_total_tokens (默认 4000)│
│                                      │
│  分配策略:                           │
│  - Full: 按需分配                    │
│  - Truncated: 每条 ≤ 200            │
│  - Summary: 每条 ≤ 80               │
│  - DenseProxy: 每条 ≤ 30            │
│                                      │
│  降级策略:                           │
│  1. 先移除 DenseProxy 结果           │
│  2. Summary → DenseProxy             │
│  3. Truncated → Summary              │
│  4. Full → Truncated                 │
│  5. 最后手段: 减少 Full 结果数量     │
│                                      │
└─────────────────────────────────────┘
```

**预算分配示例**:

```
总预算: 4000 tokens

结果列表:
- 2 条 Full (avg 300 tokens) = 600
- 5 条 Truncated (200 tokens) = 1000
- 8 条 Summary (80 tokens) = 640
- 10 条 DenseProxy (30 tokens) = 300
───────────────────────────────
总计: 2540 tokens (在预算内)
```

## 存储设计

### 预计算摘要存储

| Column Family | Key | Value | 说明 |
|---------------|-----|-------|------|
| memory_summaries | memory_id | String | 预计算摘要 |
| memory_keywords | memory_id | Vec<String> | 预计算关键词 |

### 摘要生成时机

| 时机 | 说明 |
|------|------|
| 添加记忆时 | 立即生成摘要和关键词 |
| Dream 整合时 | 重新生成摘要 |
| Facet 更新时 | 更新关键词 |

## 接口设计

### 分层检索

```
POST /api/memories/tiered-search

Request:
{
  "query": "异步编程方案",
  "top_k": 25,
  "max_total_tokens": 4000,
  "tier_thresholds": {
    "full": 0.8,
    "truncated": 0.5,
    "summary": 0.3
  }
}

Response:
{
  "total_results": 25,
  "total_tokens": 2540,
  "budget": 4000,
  "results": [
    {
      "memory_id": "uuid-1",
      "tier": "Full",
      "content": "完整内容...",
      "score": 0.92,
      "token_count": 350
    },
    {
      "memory_id": "uuid-2",
      "tier": "Truncated",
      "content": "截断内容...[截断]",
      "score": 0.65,
      "token_count": 200
    },
    {
      "memory_id": "uuid-3",
      "tier": "Summary",
      "content": "讨论了 Rust 异步编程的性能优化策略",
      "score": 0.42,
      "token_count": 80
    },
    {
      "memory_id": "uuid-4",
      "tier": "DenseProxy",
      "content": "[async, runtime, performance, pin]",
      "score": 0.18,
      "token_count": 30
    }
  ]
}
```

### 获取记忆摘要

```
GET /api/memories/{memory_id}/summary

Response:
{
  "memory_id": "uuid-xxx",
  "summary": "讨论了 Rust 异步编程的性能优化策略",
  "keywords": ["async", "runtime", "performance"],
  "token_count": 80
}
```

## 性能考量

### 分层延迟

| 操作 | 延迟 |
|------|------|
| Tier 分配 | < 1ms |
| 截断 | < 1ms |
| 摘要（预计算） | < 1ms |
| 摘要（实时） | 200-500ms |
| DenseProxy | < 1ms |
| 预算控制 | < 1ms |

### 存储开销

- 每条记忆额外存储摘要 ~100 字节 + 关键词 ~50 字节
- 10 万条记忆额外 ~15 MB

### LLM 调用成本

- 预计算摘要：每条记忆 1 次（添加时）
- Input: ~500 tokens, Output: ~80 tokens
- 检索时无额外 LLM 调用

## 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| 摘要质量损失 | Full tier 保留完整内容，摘要仅用于远距离 |
| 截断丢失关键信息 | 按句子边界截断，保留 Facet 主题 |
| 预算控制过于激进 | 优先保证 Full tier 完整，从低 tier 开始降级 |
| 预计算摘要过时 | Dream 整合时重新生成 |

## 验收标准

1. 4 层 RepresentationTier 已定义
2. 分层检索接口可用，Token 预算可控
3. 预计算摘要机制已实现
4. DenseProxy 关键词提取可用
5. 预算降级策略正确执行
6. 分层检索延迟 < 5ms（预计算模式）

## 参考资料

- MemRec 论文: https://arxiv.org/abs/2601.08816
- 论文 Section 2.1.3: Tiered Memory Representation
- 论文 Figure 6: Token Budget Allocation
- 论文 Table 3: Representation Tier Performance
- 论文 Section 4.4: Ablation on Tiered Representation
