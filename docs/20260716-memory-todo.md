# MemRec 待优化内容（基于 Grok-Build 分析）

> 来源：`hydrakiller/docs/grok-build/deconstruct/algorithm_memory/algorithm.md`
> 日期：2026-07-16

## 概览

基于对 Grok-Build Memory 系统的深入分析，识别出以下关键优化点，可显著提升 memrec 的检索质量和智能化水平。

| 优先级 | 特性 | 当前状态 | 参考 |
|--------|------|----------|------|
| P0 | MMR 重排 | 缺失 | algorithm_memory/algorithm.md#7 |
| P0 | 混合检索（KNN + BM25） | 仅 KNN | algorithm_memory/algorithm.md#6 |
| P1 | 时间衰减 | 已有（ImportanceCalculator） | algorithm_memory/algorithm.md#6.4 |
| P1 | 源权重 | 缺失 | algorithm_memory/algorithm.md#6.5 |
| P2 | Dream 后台整合 | 缺失 | algorithm_memory/algorithm.md#8 |
| P2 | DreamLock 并发控制 | 缺失 | algorithm_memory/algorithm.md#9 |

---

## 1. MMR 多样性重排（P0）

### 1.1 背景

当前检索返回的结果可能高度相似，用户看到的是重复信息。MMR（Maximal Marginal Relevance）在相关性与多样性之间取得平衡，迭代选择结果。

### 1.2 算法公式

```
MMR(d) = λ × rel(d) - (1-λ) × max(sim(d, d')) for d' in S
```

其中：
- d: 候选文档
- S: 已选集合
- rel(d): 相关性分数（即 search score）
- sim(d, d'): 文档间相似度（Jaccard）
- λ: 相关性-多样性权衡（默认 0.5）

### 1.3 实现方案

**新增文件**: `memrecd/src/search/mmr.rs`

```rust
pub struct MmrConfig {
    pub lambda: f64,           // 相关性权重，默认 0.5
    pub top_k: usize,          // 返回数量
    pub max_candidates: usize,  // 候选池大小
}

pub fn mmr_rerank(
    candidates: &mut Vec<SearchHit>,
    lambda: f64,
    limit: usize,
) -> Vec<SearchHit> {
    let mut selected: Vec<SearchHit> = Vec::new();
    
    while selected.len() < limit && !candidates.is_empty() {
        let best_idx = candidates.iter().enumerate()
            .map(|(i, c)| {
                let relevance = c.score;
                let max_sim = selected.iter()
                    .map(|s| jaccard(&tokenize(&c.text), &tokenize(&s.text)))
                    .max()
                    .unwrap_or(0.0);
                let mmr = lambda * relevance - (1.0 - lambda) * max_sim;
                (i, mmr)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(i, _)| i);
        
        if let Some(idx) = best_idx {
            selected.push(candidates.remove(idx));
        }
    }
    
    selected
}

fn tokenize(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
}
```

### 1.4 集成位置

在 `VectorStore::search()` 返回结果后，调用 `mmr_rerank()` 进行重排。

### 1.5 复杂度

| 操作 | 时间复杂度 | 说明 |
|------|-----------|------|
| tokenize | O(t) | t = 文本长度 |
| Jaccard | O(|A| + |B|) | 集合运算 |
| 全部 MMR | O(k × n²) | n 通常 10-50，可接受 |

---

## 2. 混合检索（KNN + BM25）（P0）

### 2.1 背景

当前仅有向量检索（KNN），缺乏词汇检索能力。混合检索结合语义相似性和词汇匹配，显著提升召回率。

### 2.2 流水线

```
查询 → [嵌入生成] → [KNN向量检索] ──┐
                                    ├── [合并 by ID] → [归一化] → [时间衰减] → [源权重] → [MMR重排] → 结果
查询文本 ──────────→ [FTS5 BM25] ──┘
```

### 2.3 实现方案

**方案 A**: 使用 SQLite FTS5（推荐）

```rust
pub struct HybridSearcher {
    vector_store: Arc<dyn VectorStorage>,
    fts_store: Arc<FtsStore>,  // 新增
    config: HybridConfig,
}

pub struct HybridConfig {
    pub alpha: f64,  // 向量权重，默认 0.5
    pub top_k: usize,
}

impl HybridSearcher {
    pub async fn search(&self, query: &str) -> Result<Vec<SearchHit>> {
        let query_embedding = self.embedding.embed(query)?;
        
        let vec_results = self.vector_store.search(&query_embedding, ..., top_k).await?;
        let fts_results = self.fts_store.search_bm25(query, top_k).await?;
        
        let merged = self.merge_and_normalize(&vec_results, &fts_results);
        let decayed = self.apply_time_decay(merged);
        Ok(decayed)
    }
}
```

**方案 B**: 使用 Tantivy（Rust 原生）

若不想引入 SQLite 依赖，可考虑 tantivy 全文检索库。

### 2.4 归一化公式

```
s_vec_norm = 1 - (d_cos - d_min) / (d_max - d_min)
s_fts_norm = (s_bm25 - s_min) / (s_max - s_min)
```

### 2.5 复杂度

| 阶段 | 时间复杂度 | 说明 |
|------|-----------|------|
| KNN | O(n × d) | 向量暴力扫描 |
| FTS5 | O(k × log n) | 倒排索引 |
| 合并 | O(k) | k = 候选数 |

---

## 3. 时间衰减增强（P1）

### 3.1 现状

已有 `ImportanceCalculator` 实现时间衰减：
- `calculate_recency()`: e^(-λ × 天数)
- 默认 λ = 0.05（约 14 天半衰期）

### 3.2 优化点

**常青源豁免**: Global 和 Workspace 作用域的内容不受时间衰减影响。

```rust
pub struct DecayConfig {
    pub half_life_hours: f64,  // 半衰期（小时）
    pub evergreen_scopes: Vec<MemoryScope>,  // 常青作用域
}

impl ImportanceCalculator {
    fn apply_decay(&self, score: f64, memory: &Memory) -> f64 {
        if self.config.evergreen_scopes.contains(&memory.scope) {
            return score;  // 豁免
        }
        let age_hours = (Utc::now() - memory.created_at).num_hours() as f64;
        let lambda = std::f64::consts::LN_2 / self.config.half_life_hours;
        score * (-lambda * age_hours).exp()
    }
}
```

---

## 4. 源权重（P1）

### 4.1 背景

不同来源的记忆可信度不同，应在最终评分中体现。

### 4.2 实现方案

```rust
pub struct SourceWeights {
    pub user: f64,       // 用户显式记录：1.0
    pub system: f64,     // 系统生成：0.8
    pub inferred: f64,   // 推断得出：0.5
    pub external: f64,   // 外部导入：0.7
}

fn apply_source_weight(score: f64, source: &MemorySource, weights: &SourceWeights) -> f64 {
    let w = match source {
        MemorySource::User => weights.user,
        MemorySource::System => weights.system,
        MemorySource::Inferred => weights.inferred,
        MemorySource::External => weights.external,
    };
    score * w
}
```

### 4.3 完整评分公式

```
s_final(d) = (α × s_vec_norm(d) + (1-α) × s_fts_norm(d)) × e^(-λ × age(d)) × w_source(d)
```

---

## 5. Dream 后台整合（P2）

### 5.1 背景

记忆积累后，需要后台压缩和整合，类似人类睡眠中的记忆巩固过程。

### 5.2 门控检查

Dream 触发需依次通过三个门控：

```
Gate 1: config.enabled → DreamGate::Disabled
Gate 2: time_gate (min_hours) → DreamGate::TooSoon
Gate 3: session_gate (min_sessions) → DreamGate::NotEnoughSessions
```

### 5.3 实现方案

**新增文件**: `memrecd/src/dream/mod.rs`, `memrecd/src/dream/processor.rs`

```rust
pub struct DreamProcessor {
    llm: LlmClient,  // 或 HTTP 客户端
    config: DreamConfig,
}

pub struct DreamConfig {
    pub enabled: bool,
    pub min_memories: usize,      // 触发阈值（默认 20）
    pub max_age_hours: u64,       // 最大记忆年龄（默认 168h = 7天）
    pub min_hours_between: f64,   // 最小间隔（默认 24h）
    pub integration_prompt: String,
}

impl DreamProcessor {
    pub async fn process(&self, store: &MemoryStore) -> Result<Vec<Uuid>> {
        let old_memories = store.get_older_than(self.config.max_age_hours).await?;
        
        if old_memories.len() < self.config.min_memories {
            return Ok(vec![]);
        }
        
        let summary = self.summarize(&old_memories).await?;
        let integrated = store.add(&summary, MemoryType::Knowledge).await?;
        
        for m in &old_memories {
            store.delete(&m.id).await?;
        }
        
        Ok(vec![integrated])
    }
    
    async fn summarize(&self, memories: &[Memory]) -> Result<String> {
        let prompt = self.build_prompt(memories);
        self.llm.complete(&prompt).await
    }
}
```

### 5.4 Prompt 模板

```text
你是记忆整合助手。请对以下历史记忆进行提炼：
1. 提取关键信息
2. 建立关联
3. 去除冗余
4. 生成简洁摘要

输入记忆：
{memories}

输出格式：结构化摘要（JSON）
```

### 5.5 触发时机

- 用户调用 `memrec dream` 命令
- 定时任务（如每天凌晨）
- 达到阈值时自动触发（可选）

---

## 6. DreamLock 并发控制（P2）

### 6.1 背景

防止多个 Dream 进程并发执行，导致重复整合。

### 6.2 实现方案

**新增文件**: `memrecd/src/dream/lock.rs`

```rust
pub struct DreamLock {
    lock_path: PathBuf,  // ~/.memrec/dream.lock
}

pub struct LockContent {
    pub pid: u32,
    pub timestamp: i64,
    pub session_count: u32,
}

impl DreamLock {
    pub fn try_acquire(&self) -> Result<bool> {
        if self.lock_path.exists() {
            let content = self.read_lock()?;
            if self.is_process_alive(content.pid) {
                return Ok(false);  // 已锁定
            }
            self.cleanup_stale_lock()?;
        }
        self.write_lock()?;
        Ok(true)
    }
    
    fn is_process_alive(&self, pid: u32) -> bool {
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }
    
    fn cleanup_stale_lock(&self) -> Result<()> {
        std::fs::remove_file(&self.lock_path)?;
        Ok(())
    }
}
```

### 6.3 僵锁检测

若锁文件存在但持有进程已退出（PID 不存在），则视为僵锁并自动清理。

---

## 7. 实施计划

### Phase 1（P0，预计 1 周）

| 步骤 | 改动 | 文件 |
|------|------|------|
| 1 | 实现 MMR 重排 | `search/mmr.rs` |
| 2 | 集成到搜索流程 | `storage/vector_store.rs` 或新增 `search/hybrid.rs` |
| 3 | 添加 BM25 检索 | `storage/fts_store.rs`（SQLite FTS5）或 tantivy |
| 4 | 混合检索合并 | `search/hybrid.rs` |

### Phase 2（P1，预计 3 天）

| 步骤 | 改动 | 文件 |
|------|------|------|
| 1 | 时间衰减常青豁免 | `importance/calculator.rs` |
| 2 | 源权重机制 | 新增 `search/source_weight.rs` |

### Phase 3（P2，预计 1 周）

| 步骤 | 改动 | 文件 |
|------|------|------|
| 1 | Dream 处理器 | `dream/processor.rs` |
| 2 | DreamLock 锁 | `dream/lock.rs` |
| 3 | CLI 命令 | `memrec/src/commands/dream.rs` |

---

## 8. 风险与注意事项

1. **SQLite 依赖**: 若选择 FTS5 方案，需引入 sqlite 静态链接（rusqlite 已支持）
2. **LLM 调用成本**: Dream 需要 LLM 调用，应控制频率和批大小
3. **并发安全**: DreamLock 需正确处理 SIGKILL 等异常退出场景
4. **测试覆盖**: MMR、混合检索需充分单元测试和集成测试

---

## 附录：参考文件

- `hydrakiller/docs/grok-build/deconstruct/algorithm_memory/algorithm.md` — Grok-Build Memory 算法详解
- `hydrakiller/docs/enhancement-20260716-001.md` — Hydrakiller 补强方案
- `memrec/MEMORY.md` — MemRec 项目记忆
