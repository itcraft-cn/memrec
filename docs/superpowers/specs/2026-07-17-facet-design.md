# Facet 结构化记忆设计

## 参考来源

- **论文**: MemRec: Collaborative Memory-Augmented Agentic Recommender System
- **论文 URL**: https://arxiv.org/abs/2601.08816
- **论文解读**: https://papernotes.org/ACL2026/recommender/memrec_collaborative_memory-augmented_agentic_recommender_system/
- **GitHub**: https://github.com/rutgerswiselab/memrec

## 背景与动机

### 当前问题

我们的 MemRec 记忆是扁平文本，检索时仅依赖向量相似度，存在以下问题：

1. **语义模糊**: 一条记忆可能包含多个主题，向量表示模糊
2. **检索精度低**: 纯向量相似度无法区分主题相关性
3. **缺乏结构**: 无法快速定位记忆的关键主题

### 论文启发

论文 MemRec 提出 **Facet** 结构化记忆，每个 facet 包含：
- `theme`: 主题描述（如 "偏好 Rust 异步编程"）
- `confidence`: 置信度（0-1）
- `evidence`: 支持证据（来源记忆 ID 列表）

Facet 将扁平记忆提炼为结构化主题，提升检索精度。

## 设计目标

1. 引入 MemoryFacet 结构，支持记忆的多主题表达
2. Facet 由 LLM 自动生成，无需人工标注
3. 检索时按 facet 主题匹配，提升精度
4. 存储层高效支持 facet 查询

## 概念定义

### MemoryFacet

记忆的主题维度，描述记忆的一个方面。

**属性**:

| 属性 | 类型 | 说明 |
|------|------|------|
| id | Uuid | Facet 唯一标识 |
| memory_id | Uuid | 所属记忆 ID |
| theme | String | 主题描述，自然语言 |
| confidence | f32 | 置信度，0.0-1.0 |
| evidence | Vec<Uuid> | 来源记忆 ID 列表 |
| keywords | Vec<String> | 关键词标签 |
| embedding | Vec<f32> | 主题向量 |
| created_at | DateTime | 创建时间 |
| last_accessed | DateTime | 最后访问时间 |

**示例**:

```json
{
  "id": "facet-uuid-xxx",
  "memory_id": "memory-uuid-yyy",
  "theme": "偏好使用 Tokio 进行异步编程",
  "confidence": 0.85,
  "evidence": ["uuid-1", "uuid-2"],
  "keywords": ["tokio", "async", "runtime"],
  "created_at": "2026-07-17T10:00:00Z",
  "last_accessed": "2026-07-17T12:00:00Z"
}
```

### Memory 与 Facet 关系

一条记忆可包含多个 Facet：

```
Memory (uuid-123)
├── Facet 1: "偏好 Rust 异步编程" (confidence: 0.9)
├── Facet 2: "使用 Tokio runtime" (confidence: 0.85)
└── Facet 3: "关注性能优化" (confidence: 0.7)
```

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────┐
│                    Memory Store                      │
├─────────────────────────────────────────────────────┤
│  Memory 1 ──┬── Facet 1: theme, confidence, ...    │
│             ├── Facet 2: theme, confidence, ...    │
│             └── Facet 3: theme, confidence, ...    │
├─────────────────────────────────────────────────────┤
│  Memory 2 ──┬── Facet 1: ...                       │
│             └── Facet 2: ...                       │
└─────────────────────────────────────────────────────┘
```

### 数据模型

```
┌──────────────────┐       ┌──────────────────┐
│     Memory       │       │   MemoryFacet    │
├──────────────────┤       ├──────────────────┤
│ id: Uuid         │──────▶│ id: Uuid         │
│ content: String  │       │ memory_id: Fk    │
│ memory_type: Enum│       │ theme: String    │
│ project: String  │       │ confidence: f32  │
│ tags: Vec<String>│       │ evidence: Vec<Uuid>│
│ embedding: Vec<f32>│     │ keywords: Vec<String>│
│ created_at: Dt   │       │ embedding: Vec<f32>│
│ updated_at: Dt   │       │ created_at: Dt   │
└──────────────────┘       │ last_accessed: Dt│
                           └──────────────────┘
```

### 关键设计点

1. **Facet 独立存储**: Facet 作为独立实体，支持高效查询
2. **Facet Embedding**: 每个 facet 单独计算向量，用于主题检索
3. **Evidence 追踪**: 记录来源记忆，支持溯源验证

## 流程设计

### Facet 生成流程

**触发条件**: 添加记忆时自动生成

```
┌─────────────┐
│ 用户添加记忆 │
└──────┬──────┘
       │
       ▼
┌─────────────────┐
│ 调用 LLM 分析记忆│
│ 提取 N_f 个主题  │
└──────┬──────────┘
       │
       ▼
┌─────────────────┐
│ 生成 Facet 结构  │
│ - theme         │
│ - confidence    │
│ - keywords      │
└──────┬──────────┘
       │
       ▼
┌─────────────────┐
│ 计算 Facet 向量  │
│ 存储到 DB        │
└─────────────────┘
```

**LLM Prompt 设计**:

```
你是一个记忆分析专家。请分析以下记忆内容，提取 3-5 个主要主题。

记忆内容：
{memory_content}

请为每个主题输出：
1. theme: 简洁的主题描述（一句话）
2. confidence: 置信度（0.0-1.0）
3. keywords: 3-5 个关键词

输出 JSON 格式：
{
  "facets": [
    {
      "theme": "...",
      "confidence": 0.85,
      "keywords": ["...", "..."]
    }
  ]
}
```

### Facet 检索流程

```
┌─────────────┐
│ 用户查询请求 │
└──────┬──────┘
       │
       ▼
┌─────────────────┐
│ 计算查询向量     │
└──────┬──────────┘
       │
       ▼
┌─────────────────────────────┐
│ 在 Facet 向量空间检索        │
│ 找到 top-k 相关 facets       │
└──────┬──────────────────────┘
       │
       ▼
┌─────────────────────────────┐
│ 按 memory_id 聚合            │
│ 返回相关记忆列表             │
└─────────────────────────────┘
```

**检索策略**:

| 场景 | 策略 |
|------|------|
| 精确主题匹配 | 在 facet 向量空间检索 |
| 宽泛探索 | 在 memory 向量空间检索 |
| 混合模式 | 先 facet 检索，再 memory 补充 |

### Facet 更新流程

**触发条件**: Dream 整合时更新

```
┌─────────────────┐
│ Dream 整合触发   │
└──────┬──────────┘
       │
       ▼
┌─────────────────────────┐
│ 重新分析记忆内容         │
│ 提取更新后的 facets      │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 对比新旧 facets          │
│ - 新增 facet: 添加       │
│ - 删除 facet: 标记失效   │
│ - 更新 facet: 刷新       │
└──────┬──────────────────┘
       │
       ▼
┌─────────────────────────┐
│ 更新存储                 │
└─────────────────────────┘
```

## 存储设计

### RocksDB 列族设计

| Column Family | Key | Value | 说明 |
|---------------|-----|-------|------|
| memories | memory_id | Memory | 记忆主表 |
| facets | facet_id | MemoryFacet | Facet 主表 |
| memory_facets | memory_id | Vec<facet_id> | Memory→Facet 索引 |
| facet_embeddings | facet_id | Vec<f32> | Facet 向量索引 |

### 向量索引

使用 HNSW 索引：
- Memory 向量索引：用于宽泛检索
- Facet 向量索引：用于主题检索

## 接口设计

### 添加 Facet

```
POST /api/memories/{memory_id}/facets

Request:
{
  "theme": "偏好 Rust 异步编程",
  "confidence": 0.85,
  "keywords": ["tokio", "async"]
}

Response:
{
  "facet_id": "uuid-xxx",
  "status": "created"
}
```

### 检索 Facet

```
GET /api/facets/search?q=异步编程&top_k=10

Response:
{
  "results": [
    {
      "facet_id": "uuid-xxx",
      "memory_id": "uuid-yyy",
      "theme": "偏好 Rust 异步编程",
      "confidence": 0.85,
      "score": 0.92
    }
  ]
}
```

### 获取记忆的 Facets

```
GET /api/memories/{memory_id}/facets

Response:
{
  "memory_id": "uuid-yyy",
  "facets": [
    {
      "facet_id": "uuid-xxx",
      "theme": "偏好 Rust 异步编程",
      "confidence": 0.85,
      "keywords": ["tokio", "async"]
    }
  ]
}
```

## 性能考量

### 内存占用

- 每个 Facet 约 200-500 字节（含向量）
- 每条 Memory 平均 3-5 个 Facet
- 10 万条 Memory 约 60-250 MB Facet 数据

### 检索延迟

- Facet 向量检索：< 10ms（HNSW）
- Memory 聚合：< 5ms
- 总延迟：< 20ms

### LLM 调用成本

- 每条 Memory 调用 1 次 LLM 生成 Facet
- Input: ~500 tokens, Output: ~300 tokens
- 可批量处理降低延迟

## 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| Facet 质量不稳定 | 设计健壮的 LLM prompt，添加验证逻辑 |
| Facet 数量爆炸 | 设置上限（每条记忆最多 7 个 facet） |
| 向量索引膨胀 | 定期清理低置信度 facet |
| 检索结果过于细分 | 提供 Memory 级聚合视图 |

## 验收标准

1. MemoryFacet 结构已定义，存储层已支持
2. LLM 可自动生成高质量 Facet
3. Facet 向量检索可用，延迟 < 20ms
4. 提供 Facet CRUD 接口
5. 检索精度相对纯向量检索提升 > 10%

## 参考资料

- MemRec 论文: https://arxiv.org/abs/2601.08816
- 论文 Section 2.1.1: Collaborative Memory Retrieval
- 论文 Figure 4: Synthesis Prompt
- 论文 Table 8: Token I/O 分析
