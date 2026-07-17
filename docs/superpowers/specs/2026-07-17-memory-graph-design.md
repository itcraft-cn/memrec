# 记忆图边关系设计

## 参考来源

- **论文**: MemRec: Collaborative Memory-Augmented Agentic Recommender System
- **论文 URL**: https://arxiv.org/abs/2601.08816
- **论文解读**: https://papernotes.org/ACL2026/recommender/memrec_collaborative_memory-augmented_agentic_recommender_system/
- **GitHub**: https://github.com/rutgerswiselab/memrec

## 背景与动机

### 当前问题

我们的 MemRec 记忆之间是孤立的，缺乏关联：

1. **无关联**: 记忆之间没有显式关系，无法沿关系扩散检索
2. **上下文断裂**: 相关记忆散落各处，无法形成知识网络
3. **重复冗余**: 相似记忆无法自动关联，导致冗余存储

### 论文启发

论文 MemRec 构建协同记忆图（Collaborative Memory Graph），节点为用户/物品，边为协同关系。我们借鉴其图结构思想，但将节点改为记忆，边改为记忆间语义关系。

## 设计目标

1. 引入记忆间关联图，支持显式关系表达
2. 支持多种边类型，覆盖不同语义关系
3. 支持沿图扩散检索，发现隐式关联
4. 图存储高效，支持快速遍历

## 概念定义

### 边类型（EdgeType）

| 边类型 | 语义 | 方向 | 示例 |
|--------|------|------|------|
| Similar | 语义相似 | 无向 | "偏好Tokio" ↔ "偏好async-std" |
| CausedBy | 因果关系 | 有向 | "性能提升" ← "使用零拷贝" |
| References | 引用关系 | 有向 | "重构方案" → "原始设计" |
| SameProject | 同项目 | 无向 | 同一项目的多条记忆 |
| SameSession | 同会话 | 无向 | 同一会话产生的记忆 |
| Contradicts | 矛盾关系 | 无向 | "用PostgreSQL" ↔ "用SQLite" |
| Evolves | 演进关系 | 有向 | "v1设计" → "v2设计" |

### MemoryEdge

| 属性 | 类型 | 说明 |
|------|------|------|
| id | Uuid | 边唯一标识 |
| source_id | Uuid | 源记忆 ID |
| target_id | Uuid | 目标记忆 ID |
| edge_type | EdgeType | 边类型 |
| weight | f32 | 边权重，0.0-1.0 |
| evidence | String | 关系证据描述 |
| created_at | DateTime | 创建时间 |
| auto_generated | bool | 是否自动生成 |

### 记忆图示例

```
Memory A: "偏好Tokio异步运行时"
  ├── Similar(0.85) ──→ Memory B: "偏好async-std异步运行时"
  ├── SameProject ──→ Memory C: "项目使用Tokio 1.x"
  └── Evolves ──→ Memory D: "升级到Tokio 2.x"

Memory E: "性能优化方案：零拷贝解析"
  ├── CausedBy ←── Memory F: "解析性能瓶颈"
  └── References ──→ Memory G: "零拷贝设计文档"
```

## 架构设计

### 整体架构

```
┌──────────────────────────────────────────────────┐
│                  Memory Graph                     │
├──────────────────────────────────────────────────┤
│                                                   │
│  [M1] ──Similar── [M2] ──SameProject── [M3]     │
│    │                │                              │
│  CausedBy        References                       │
│    │                │                              │
│    ▼                ▼                              │
│  [M4] ──Evolves── [M5] ──SameSession── [M6]      │
│                                                   │
└──────────────────────────────────────────────────┘
```

### 图存储层

```
┌─────────────────────────────────────────┐
│            Graph Storage                 │
├─────────────────────────────────────────┤
│  Adjacency Index: node → Vec<Edge>      │
│  Edge Store: edge_id → MemoryEdge       │
│  Type Index: edge_type → Vec<edge_id>   │
│  Weight Index: 支持按权重过滤           │
└─────────────────────────────────────────┘
```

## 流程设计

### 边自动生成流程

**触发条件**: 添加记忆时自动发现关联

```
┌─────────────┐
│ 用户添加记忆 │
└──────┬──────┘
       │
       ▼
┌─────────────────────────┐
│ 向量检索 top-k 相似记忆  │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ LLM 判断关系类型和权重   │
│ 输入: 新记忆 + 候选记忆  │
│ 输出: EdgeType + weight  │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 过滤低权重边 (w < 0.5)  │
│ 存储有效边               │
└─────────────────────────┘
```

**LLM Prompt 设计**:

```
你是一个记忆关系分析专家。请判断以下两条记忆之间的关系。

记忆 A：{new_memory}
记忆 B：{candidate_memory}

请从以下关系中选择最合适的一个或多个：
- Similar: 语义相似
- CausedBy: A 导致 B（或 B 导致 A）
- References: A 引用 B（或 B 引用 A）
- Contradicts: A 与 B 矛盾
- Evolves: A 是 B 的演进版本（或反之）
- None: 无明显关系

输出 JSON：
{
  "relations": [
    {
      "type": "Similar",
      "weight": 0.85,
      "evidence": "两者都讨论异步运行时选择"
    }
  ]
}
```

### SameProject/SameSession 自动生成

这两类边无需 LLM，基于元数据自动生成：

```
┌─────────────────────────┐
│ 新记忆携带 project/session │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 查找同 project 的记忆    │
│ 添加 SameProject 边     │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 查找同 session 的记忆    │
│ 添加 SameSession 边     │
└─────────────────────────┘
```

### 图遍历检索流程

```
┌─────────────────┐
│ 初始检索命中 M1  │
└──────┬──────────┘
       │
       ▼
┌─────────────────────────────┐
│ 沿边扩散（BFS/DFS）          │
│ - Similar: 扩散权重 1.0     │
│ - CausedBy: 扩散权重 0.8    │
│ - References: 扩散权重 0.7  │
│ - SameProject: 扩散权重 0.5 │
│ - SameSession: 扩散权重 0.3 │
└──────┬──────────────────────┘
       │
       ▼
┌─────────────────────────────┐
│ 计算扩散得分                 │
│ score = base × edge_weight  │
│ × decay^depth               │
└──────┬──────────────────────┘
       │
       ▼
┌─────────────────────────────┐
│ 去重、排序、返回             │
└─────────────────────────────┘
```

**扩散参数**:

| 参数 | 默认值 | 说明 |
|------|--------|------|
| max_depth | 3 | 最大扩散深度 |
| decay | 0.6 | 每层衰减系数 |
| min_score | 0.3 | 最低扩散得分阈值 |
| max_results | 50 | 最大返回结果数 |

### 图维护流程

**触发条件**: Dream 整合时

```
┌─────────────────┐
│ Dream 整合压缩   │
│ 旧记忆被合并     │
└──────┬──────────┘
       │
       ▼
┌─────────────────────────────┐
│ 更新边关系                   │
│ - 旧记忆的边迁移到新记忆     │
│ - 重新评估边权重             │
│ - 删除失效边                 │
└─────────────────────────────┘
```

## 存储设计

### RocksDB 列族设计

| Column Family | Key | Value | 说明 |
|---------------|-----|-------|------|
| edges | edge_id | MemoryEdge | 边主表 |
| node_edges_out | (source_id, edge_type) | Vec<edge_id> | 出边索引 |
| node_edges_in | (target_id, edge_type) | Vec<edge_id> | 入边索引 |
| edges_by_type | edge_type | Vec<edge_id> | 按类型索引 |

### 邻接表结构

```
node_edges_out:{source_id}:{edge_type} → [edge_id_1, edge_id_2, ...]
node_edges_in:{target_id}:{edge_type}  → [edge_id_1, edge_id_2, ...]
```

## 接口设计

### 添加边

```
POST /api/memories/edges

Request:
{
  "source_id": "uuid-a",
  "target_id": "uuid-b",
  "edge_type": "Similar",
  "weight": 0.85,
  "evidence": "两者都讨论异步运行时选择"
}

Response:
{
  "edge_id": "uuid-xxx",
  "status": "created"
}
```

### 查询邻居

```
GET /api/memories/{memory_id}/neighbors?edge_type=Similar&max_depth=2

Response:
{
  "memory_id": "uuid-a",
  "neighbors": [
    {
      "memory_id": "uuid-b",
      "edge_type": "Similar",
      "weight": 0.85,
      "path": ["uuid-a", "uuid-b"],
      "score": 0.85
    },
    {
      "memory_id": "uuid-c",
      "edge_type": "SameProject",
      "weight": 1.0,
      "path": ["uuid-a", "uuid-b", "uuid-c"],
      "score": 0.51
    }
  ]
}
```

### 图扩散检索

```
POST /api/memories/graph-search

Request:
{
  "seed_memory_ids": ["uuid-a"],
  "edge_types": ["Similar", "CausedBy"],
  "max_depth": 3,
  "min_score": 0.3,
  "max_results": 20
}

Response:
{
  "results": [
    {
      "memory_id": "uuid-b",
      "content": "...",
      "score": 0.85,
      "path": ["uuid-a → uuid-b"],
      "path_types": ["Similar"]
    }
  ]
}
```

## 性能考量

### 存储开销

- 每条边约 100-200 字节
- 每条 Memory 平均 5-10 条边
- 10 万条 Memory 约 50-200 MB 边数据

### 遍历延迟

- 单跳邻居查询：< 5ms（邻接表索引）
- 2 跳扩散：< 20ms
- 3 跳扩散：< 50ms

### LLM 调用成本

- 每条新记忆对 top-k 候选调用 1 次 LLM
- Input: ~800 tokens, Output: ~200 tokens
- 可批量处理

## 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| 边数量爆炸 | 设置每节点最大边数（≤20），低权重边自动清理 |
| 图遍历性能 | 限制扩散深度，使用邻接表索引 |
| 边质量不稳定 | LLM 生成后验证，低权重边不存储 |
| 图维护复杂 | Dream 整合时批量更新，异步执行 |

## 验收标准

1. MemoryEdge 结构已定义，存储层已支持
2. 7 种边类型已实现
3. SameProject/SameSession 自动生成
4. LLM 可自动发现语义关系
5. 图遍历检索可用，2 跳延迟 < 20ms
6. 图扩散检索可用，3 跳延迟 < 50ms

## 参考资料

- MemRec 论文: https://arxiv.org/abs/2601.08816
- 论文 Section 2.1: Collaborative Memory Graph
- 论文 Figure 2: Memory Graph Structure
- 论文 Section 3.2: Asynchronous Batch Propagation
