# MemRec 搜索补强设计文档

> 日期：2026-07-16
> 状态：待实施
> 范围：P0（MMR重排 + 混合检索） + P1（时间衰减常青豁免 + 源权重）

## 1. 背景

当前 MemRec 仅支持纯向量检索（KNN），存在以下问题：

1. **结果相似度过高**：返回的记忆内容高度重复，缺乏多样性
2. **词汇匹配缺失**：无法精确匹配关键词，召回率不足
3. **时间衰减一刀切**：全局记忆（用户偏好）也被衰减，不合理
4. **来源可信度未体现**：用户显式记录与系统推断应有不同权重

基于 Grok-Build Memory 系统分析，本设计实施 P0 + P1 补强。

## 2. 目标

1. 提升**检索多样性**：MMR 重排降低重复结果
2. 提升**召回率**：混合检索（KNN + BM25）结合语义与词汇
3. 提升**结果相关性**：时间衰减常青豁免 + 源权重机制
4. 保持**向后兼容**：现有数据和接口平稳迁移

## 3. 架构设计

### 3.1 整体架构

```
┌─────────────────────────────────────────────────────────┐
│                      Handler Layer                       │
│  (server/handler.rs: handle_search_memory)               │
└─────────────────────┬───────────────────────────────────┘
                      │ HybridSearchRequest
                      ▼
┌─────────────────────────────────────────────────────────┐
│                   HybridStore                            │
│  (storage/hybrid_store.rs)                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │VectorStorage│  │ FtsStorage  │  │ Search Scorer   │  │
│  │  (KNN)      │  │  (BM25)     │  │ (decay+weight)  │  │
│  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘  │
│         │                │                   │           │
│         ▼                ▼                   ▼           │
│    merge & normalize → apply scoring → MMR rerank       │
└─────────────────────────────────────────────────────────┘
         │                      │
         ▼                      ▼
┌─────────────────┐    ┌─────────────────┐
│RocksDBVectorStore│    │ TantivyStore    │
│ (~/.memrec/vectors)│   │(~/.memrec/fts)  │
└─────────────────┘    └─────────────────┘
```

### 3.2 模块划分

```
memrecd/src/
├── storage/
│   ├── traits.rs           # 新增 FtsStorage/HybridStorage trait
│   ├── hybrid_store.rs     # HybridStore 实现（新增）
│   ├── tantivy_store.rs    # TantivyStore 实现（新增）
│   ├── vector_store.rs     # 不变
│   ├── rocksdb_vector.rs   # 不变
│   └── memory_store.rs     # 扩展 source/scope 支持
├── search/
│   ├── mod.rs              # 搜索模块入口（新增）
│   ├── mmr.rs              # MMR 重排算法（新增）
│   └── scorer.rs           # 时间衰减 + 源权重（新增）
└── server/
    └── handler.rs          # 集成 HybridStore

common/src/types/
└── memory.rs               # 新增 MemorySource/MemoryScope
```

## 4. 核心组件设计

### 4.1 Trait 定义

**FtsStorage trait**（storage/traits.rs）：

```rust
#[async_trait]
pub trait FtsStorage: Send + Sync {
    async fn add(&self, id: &Uuid, text: &str, payload: FtsPayload) -> Result<()>;
    async fn remove(&self, id: &Uuid) -> Result<bool>;
    async fn search(&self, query: &str, filter: SearchFilter, top_k: usize) -> Result<Vec<SearchHit>>;
    async fn count(&self) -> Result<usize>;
}

pub struct FtsPayload {
    pub project_id: Option<Uuid>,
    pub memory_type: String,
    pub tags: Vec<String>,
}
```

**HybridStorage trait**（storage/traits.rs）：

```rust
#[async_trait]
pub trait HybridStorage: Send + Sync {
    async fn search(&self, req: HybridSearchRequest) -> Result<HybridSearchResult>;
    async fn add(&self, memory: &Memory) -> Result<()>;
    async fn remove(&self, id: &Uuid) -> Result<bool>;
}

pub struct HybridSearchRequest {
    pub query: String,
    pub query_embedding: Vec<f32>,
    pub filter: SearchFilter,
    pub top_k: usize,
    pub hybrid_alpha: f64,
    pub mmr_lambda: f64,
    pub mmr_enabled: bool,
}

pub struct HybridSearchResult {
    pub hits: Vec<SearchHit>,
    pub vec_count: usize,
    pub fts_count: usize,
}
```

### 4.2 HybridStore

**结构定义**（storage/hybrid_store.rs）：

```rust
pub struct HybridStore {
    vector_store: Arc<dyn VectorStorage>,
    fts_store: Arc<dyn FtsStorage>,
    config: HybridConfig,
    scorer_config: ScorerConfig,
    mmr_config: MmrConfig,
}

pub struct HybridConfig {
    pub alpha: f64,
    pub top_k: usize,
    pub mmr_enabled: bool,
}
```

**search 流程**：

```rust
impl HybridStorage for HybridStore {
    async fn search(&self, req: HybridSearchRequest) -> Result<HybridSearchResult> {
        // 1. 并行执行 KNN 和 BM25
        let (vec_hits, fts_hits) = tokio::join!(
            self.vector_store.search(&req.query_embedding, req.filter.clone(), req.top_k),
            self.fts_store.search(&req.query, req.filter, req.top_k)
        );
        
        // 2. 合并归一化
        let merged = self.merge_and_normalize(vec_hits?, fts_hits?, req.hybrid_alpha);
        
        // 3. 评分：时间衰减 + 源权重
        let scored = self.apply_scoring(merged).await?;
        
        // 4. MMR 重排
        let final_hits = if req.mmr_enabled {
            mmr_rerank(scored, &self.mmr_config)
        } else {
            scored
        };
        
        Ok(HybridSearchResult { hits: final_hits, vec_count, fts_count })
    }
}
```

**归一化公式**：

```
s_vec_norm = 1 - (d - d_min) / (d_max - d_min)   // 距离转相似度
s_fts_norm = (s - s_min) / (s_max - s_min)       // BM25 分数归一化

s_merged = α × s_vec_norm + (1-α) × s_fts_norm
```

### 4.3 TantivyStore

**索引结构**（storage/tantivy_store.rs）：

```rust
pub struct TantivyStore {
    index: Index,
    writer: IndexWriter,
    reader: IndexReader,
    schema: Schema,
}
```

**Schema 定义**：

| 字段 | 类型 | 用途 |
|------|------|------|
| id | STORED | Uuid 字符串，唯一标识 |
| content | TEXT | 全文索引 |
| project_id | STORED | 过滤 |
| memory_type | STORED | 过滤 |
| tags | TEXT | 标签搜索 |
| created_at | FAST(I64) | 排序 |
| importance | FAST(F64) | 排序 |

**数据目录**：`~/.memrec/fts/`

### 4.4 MMR 重排

**算法**（search/mmr.rs）：

```
MMR(d) = λ × rel(d) - (1-λ) × max(sim(d, d')) for d' in S
```

- `rel(d)`：候选文档相关性分数（search score）
- `sim(d, d')`：文档间 Jaccard 相似度
- `λ`：相关性-多样性权衡（默认 0.5）

**配置**：

```rust
pub struct MmrConfig {
    pub lambda: f64,
    pub top_k: usize,
    pub max_candidates: usize,
}
```

**复杂度**：O(k × n²)，n 通常 10-50，可接受。

### 4.5 评分器

**结构**（search/scorer.rs）：

```rust
pub struct ScorerConfig {
    pub decay_half_life_hours: f64,
    pub evergreen_scopes: Vec<MemoryScope>,
    pub source_weights: SourceWeights,
}

pub struct SourceWeights {
    pub user: f64,       // 1.0
    pub system: f64,     // 0.8
    pub inferred: f64,   // 0.5
    pub external: f64,   // 0.7
}
```

**完整评分公式**：

```
s_final = s_merged × decay × w_source

decay = e^(-λ × age_hours)  if scope not in evergreen_scopes
       = 1.0                 otherwise

λ = ln(2) / half_life_hours
```

## 5. 类型扩展

### 5.1 MemorySource

**新增枚举**（common/src/types/memory.rs）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemorySource {
    #[default]
    User,      // 用户显式记录
    System,    // 系统自动生成
    Inferred,  // 推断得出
    External,  // 外部导入
}
```

**权重映射**：

| Source | 权重 | 说明 |
|--------|------|------|
| User | 1.0 | 最高可信度 |
| System | 0.8 | 系统生成 |
| External | 0.7 | 外部导入 |
| Inferred | 0.5 | 推断得出 |

### 5.2 MemoryScope

**新增枚举**（common/src/types/memory.rs）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    #[default]
    Project,   // 项目隔离
    Global,    // 全局共享
    Workspace, // 工作区共享
}
```

**衰减豁免规则**：

| Scope | 是否衰减 | 说明 |
|-------|---------|------|
| Global | 否 | 用户偏好等常青内容 |
| Workspace | 否 | 工作区共享知识 |
| Project | 是 | 项目特定记忆 |

### 5.3 Memory 结构体扩展

```rust
pub struct Memory {
    // ... 现有字段 ...
    
    #[serde(default)]
    pub source: MemorySource,
    
    #[serde(default)]
    pub scope: MemoryScope,
}
```

**向后兼容**：
- 现有数据无 `source`/`scope` 字段，反序列化使用 `#[serde(default)]`
- 默认值：`MemorySource::User`，`MemoryScope::Project`

## 6. 集成改动

### 6.1 Handler 改动

**Router 结构体**（server/handler.rs）：

```rust
pub struct Router {
    storage: Arc<dyn MemoryStorage>,
    hybrid_store: Arc<dyn HybridStorage>,  // 替换原 vector_store
    embedder: Arc<dyn EmbeddingGenerator>,
}
```

**handle_search_memory**：

```rust
async fn handle_search_memory(&self, p: SearchMemoryParams) -> Result<ResponseResult> {
    let query_embedding = self.embedder.embed(&p.query)?;
    
    let req = HybridSearchRequest {
        query: p.query,
        query_embedding,
        filter: self.build_filter(&p),
        top_k: p.limit.unwrap_or(10),
        hybrid_alpha: 0.5,
        mmr_lambda: 0.5,
        mmr_enabled: true,
    };
    
    let result = self.hybrid_store.search(req).await?;
    // 补充元数据，返回结果
}
```

### 6.2 数据写入流程

**handle_add_memory**：

```rust
async fn handle_add_memory(&self, p: AddMemoryParams) -> Result<ResponseResult> {
    let memory = Memory {
        source: p.source.unwrap_or_default(),
        scope: p.scope.unwrap_or_default(),
        // ...
    };
    
    self.storage.add(&memory).await?;
    self.hybrid_store.add(&memory).await?;
    
    Ok(ResponseResult::AddMemory(AddMemoryResult { id: memory.id }))
}
```

### 6.3 RequestParams 扩展

**AddParams 扩展**（common/src/protocol/request.rs）：

```rust
pub struct AddParams {
    pub content: String,
    // ... 现有字段 ...
    
    #[serde(default)]
    pub source: Option<MemorySource>,  // 新增：记忆来源
    
    #[serde(default)]
    pub scope: Option<MemoryScope>,    // 新增：记忆作用域
}
```

**SearchMemoryParams 扩展**（common/src/protocol/request.rs）：

```rust
pub struct SearchMemoryParams {
    pub query: String,
    // ... 现有字段 ...
    
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,             // 新增：向量权重
    
    #[serde(default = "default_mmr_enabled")]
    pub mmr_enabled: bool,             // 新增：是否启用 MMR
    
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,               // 新增：MMR 相关性权重
}

pub fn default_hybrid_alpha() -> f64 { 0.5 }
pub fn default_mmr_enabled() -> bool { true }
pub fn default_mmr_lambda() -> f64 { 0.5 }
```

### 6.4 配置扩展

**新增 SearchConfig**（common/src/types/config.rs）：

```rust
pub struct SearchConfig {
    pub hybrid_alpha: f64,           // 默认 0.5
    pub mmr_enabled: bool,           // 默认 true
    pub mmr_lambda: f64,             // 默认 0.5
    pub decay_half_life_hours: f64,  // 默认 336（14天）
    pub source_weights: SourceWeights,
}
```

## 7. 数据流

### 7.1 搜索流程

```
用户查询 "认证方案"
    │
    ▼
embedder.embed() → Vec<f32>
    │
    ▼
HybridStore::search()
    ├─ vector_store.search() ──→ Vec<SearchHit> (KNN)
    │
    ├─ fts_store.search() ─────→ Vec<SearchHit> (BM25)
    │
    ▼
merge_and_normalize() → Vec<SearchHit>
    │
    ▼
apply_scoring()
    ├─ apply_time_decay() → 考虑 scope 豁免
    └─ apply_source_weight()
    │
    ▼
mmr_rerank() → Vec<SearchHit>
    │
    ▼
补充 Memory 元数据
    │
    ▼
返回 SemanticSearchResult
```

### 7.2 写入流程

```
用户添加记忆
    │
    ▼
MemoryStore::add() → RocksDB 元数据
    │
    ▼
HybridStore::add()
    ├─ vector_store.add() → RocksDB 向量
    └─ fts_store.add() → Tantivy 索引
```

## 8. 错误处理

| 错误场景 | 处理方式 |
|---------|---------|
| Tantivy 索引损坏 | 启动时检测，自动重建（从 MemoryStore 全量同步） |
| 向量检索失败 | 降级为纯 BM25 检索，日志警告 |
| BM25 检索失败 | 降级为纯向量检索，日志警告 |
| 双写部分失败 | 事务回滚，返回错误 |

## 9. 测试策略

### 9.1 单元测试

| 模块 | 测试内容 |
|------|---------|
| mmr.rs | Jaccard 相似度计算、MMR 迭代选择 |
| scorer.rs | 时间衰减公式、源权重应用、常青豁免 |
| hybrid_store.rs | 合并归一化、去重逻辑 |
| tantivy_store.rs | 索引增删查、过滤条件 |

### 9.2 集成测试

| 测试 | 内容 |
|------|------|
| 混合检索 | 验证 KNN + BM25 合并结果 |
| MMR 效果 | 验证多样性提升（相似文档去重） |
| 评分正确性 | 验证 decay + source weight 最终分数 |
| 向后兼容 | 现有数据读取、默认值应用 |

## 10. 实施计划

### Phase 1：类型扩展（1天）

1. 新增 `MemorySource` / `MemoryScope` 枚举
2. 扩展 `Memory` 结构体
3. 扩展 `AddParams` / `SearchMemoryParams`
4. 更新 `MemoryStore` 读写逻辑

### Phase 2：搜索算法模块（1天）

1. 实现 `search/mmr.rs`
2. 实现 `search/scorer.rs`
3. 单元测试

### Phase 3：全文检索（2天）

1. 新增 `FtsStorage` trait
2. 实现 `TantivyStore`
3. 集成测试

### Phase 4：HybridStore（1天）

1. 新增 `HybridStorage` trait
2. 实现 `HybridStore`
3. 集成到 Handler

### Phase 5：集成测试与文档（1天）

1. 全链路集成测试
2. 更新用户文档

**总计：约 6 天**

## 11. 文件改动清单

**新增文件**：

| 文件 | 说明 |
|------|------|
| `memrecd/src/storage/traits.rs` | 新增 `FtsStorage` / `HybridStorage` trait（扩展现有文件） |
| `memrecd/src/storage/hybrid_store.rs` | `HybridStore` 实现 |
| `memrecd/src/storage/tantivy_store.rs` | `TantivyStore` 实现 |
| `memrecd/src/search/mod.rs` | 搜索模块入口 |
| `memrecd/src/search/mmr.rs` | MMR 重排算法 |
| `memrecd/src/search/scorer.rs` | 时间衰减 + 源权重 |

**修改文件**：

| 文件 | 改动 |
|------|------|
| `common/src/types/memory.rs` | 新增 `MemorySource` / `MemoryScope` 枚举，扩展 `Memory` 结构体 |
| `common/src/protocol/request.rs` | 扩展 `AddParams` / `SearchMemoryParams` |
| `common/src/types/config.rs` | 新增 `SearchConfig` |
| `memrecd/src/server/handler.rs` | 集成 `HybridStore`，修改 `Router` 结构体 |
| `memrecd/src/storage/memory_store.rs` | 支持 `source` / `scope` 字段读写 |
| `memrecd/Cargo.toml` | 新增 `tantivy` 依赖 |

## 12. 风险与应对

| 风险 | 影响 | 应对 |
|------|------|------|
| Tantivy 索引与 RocksDB 不一致 | 搜索结果偏差 | 双写事务、定期校验 |
| 现有数据无 source/scope | 默认值可能导致评分偏差 | 提供迁移脚本、按需更新 |
| 性能回退 | 搜索延迟增加 | hybrid_alpha 可配置、可降级纯向量 |
| 依赖增加 | 编译时间增加 | tantivy 可选 feature |

## 13. 依赖

**新增**：

```toml
tantivy = "0.22"
```

**不变**：rocksdb, tokio, fastembed, serde, async-trait 等

## 附录：参考

- `docs/20260716-memory-todo.md` — 待优化内容
- Grok-Build Memory 算法分析
- Tantivy 官方文档：https://docs.tantivy-search.org/
