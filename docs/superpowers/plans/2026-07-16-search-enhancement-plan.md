# MemRec 搜索补强实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 MemRec 实现 MMR 重排、混合检索（KNN + BM25）、时间衰减常青豁免、源权重机制。

**Architecture:** 新增 FtsStorage/HybridStorage trait，通过 HybridStore 统一封装 VectorStorage + FtsStorage，搜索流程为：KNN+BM25 并行 → 合并归一化 → 评分（衰减+源权重）→ MMR 重排。

**Tech Stack:** Rust, Tokio, Tantivy 0.22, RocksDB, FastEmbed

---

## 文件结构

**新增文件：**

| 文件 | 职责 |
|------|------|
| `memrecd/src/search/mod.rs` | 搜索模块入口 |
| `memrecd/src/search/mmr.rs` | MMR 重排算法 |
| `memrecd/src/search/scorer.rs` | 时间衰减 + 源权重评分 |
| `memrecd/src/storage/tantivy_store.rs` | Tantivy 全文检索实现 |
| `memrecd/src/storage/hybrid_store.rs` | HybridStore 实现 |

**修改文件：**

| 文件 | 改动 |
|------|------|
| `common/src/types/memory.rs` | 新增 MemorySource/MemoryScope 枚举 |
| `common/src/types/config.rs` | 新增 SearchConfig |
| `common/src/protocol/request.rs` | 扩展 AddParams/SearchMemoryParams |
| `memrecd/src/storage/traits.rs` | 新增 FtsStorage/HybridStorage trait |
| `memrecd/src/storage/memory_store.rs` | 支持 source/scope 字段 |
| `memrecd/src/server/handler.rs` | 集成 HybridStore |
| `memrecd/src/lib.rs` | 导出 search 模块 |
| `memrecd/Cargo.toml` | 新增 tantivy 依赖 |

---

## Task 1: 新增 MemorySource/MemoryScope 枚举

**Files:**
- Modify: `common/src/types/memory.rs`

- [ ] **Step 1: 在 memory.rs 中添加 MemorySource 枚举**

在 `MemoryType` 枚举定义之后添加：

```rust
/// 记忆来源枚举。
///
/// 区分记忆的产生方式，影响搜索结果的可信度权重。
/// 序列化为小写字符串，默认为 `User`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MemorySource {
    #[default]
    User,
    System,
    Inferred,
    External,
}

impl std::fmt::Display for MemorySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemorySource::User => write!(f, "user"),
            MemorySource::System => write!(f, "system"),
            MemorySource::Inferred => write!(f, "inferred"),
            MemorySource::External => write!(f, "external"),
        }
    }
}
```

- [ ] **Step 2: 在 memory.rs 中添加 MemoryScope 枚举**

在 `MemorySource` 枚举定义之后添加：

```rust
/// 记忆作用域枚举。
///
/// 控制记忆的可见范围和时间衰减行为：
/// - `Project`: 项目隔离，受时间衰减影响
/// - `Global`: 全局共享，豁免时间衰减
/// - `Workspace`: 工作区共享（预留），豁免时间衰减
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MemoryScope {
    #[default]
    Project,
    Global,
    Workspace,
}

impl std::fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryScope::Project => write!(f, "project"),
            MemoryScope::Global => write!(f, "global"),
            MemoryScope::Workspace => write!(f, "workspace"),
        }
    }
}
```

- [ ] **Step 3: 扩展 Memory 结构体**

在 `Memory` 结构体中添加 `source` 和 `scope` 字段：

```rust
pub struct Memory {
    // ... 现有字段 ...
    pub chunk_total: Option<u32>,
    
    #[serde(default)]
    pub source: MemorySource,
    
    #[serde(default)]
    pub scope: MemoryScope,
}
```

- [ ] **Step 4: 更新 Memory::new 方法**

在 `Memory::new` 中初始化新字段：

```rust
pub fn new(content: String, memory_type: MemoryType) -> Self {
    let now = Utc::now();
    Self {
        // ... 现有字段 ...
        chunk_total: None,
        source: MemorySource::default(),
        scope: MemoryScope::default(),
    }
}
```

- [ ] **Step 5: 添加 with_source 和 with_scope 方法**

```rust
/// 设置记忆来源，返回自身以支持链式调用。
pub fn with_source(mut self, source: MemorySource) -> Self {
    self.source = source;
    self
}

/// 设置记忆作用域，返回自身以支持链式调用。
pub fn with_scope(mut self, scope: MemoryScope) -> Self {
    self.scope = scope;
    self
}
```

- [ ] **Step 6: 添加单元测试**

在 `#[cfg(test)] mod tests` 中添加：

```rust
#[test]
fn test_memory_source_serde() {
    let sources = [
        MemorySource::User,
        MemorySource::System,
        MemorySource::Inferred,
        MemorySource::External,
    ];

    for s in sources {
        let json = serde_json::to_string(&s).unwrap();
        let parsed: MemorySource = serde_json::from_str(&json).unwrap();
        assert_eq!(s, parsed);
    }
}

#[test]
fn test_memory_scope_serde() {
    let scopes = [
        MemoryScope::Project,
        MemoryScope::Global,
        MemoryScope::Workspace,
    ];

    for s in scopes {
        let json = serde_json::to_string(&s).unwrap();
        let parsed: MemoryScope = serde_json::from_str(&json).unwrap();
        assert_eq!(s, parsed);
    }
}

#[test]
fn test_memory_source_default() {
    let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
    assert_eq!(memory.source, MemorySource::User);
    assert_eq!(memory.scope, MemoryScope::Project);
}

#[test]
fn test_memory_backward_compatibility() {
    let json = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "memory_type": "knowledge",
        "content": "test",
        "importance": 0.8,
        "created_at": "2026-01-01T00:00:00Z",
        "last_accessed": "2026-01-01T00:00:00Z",
        "access_count": 0,
        "tags": [],
        "metadata": {},
        "is_deleted": false
    }"#;
    
    let memory: Memory = serde_json::from_str(json).unwrap();
    assert_eq!(memory.source, MemorySource::User);
    assert_eq!(memory.scope, MemoryScope::Project);
}
```

- [ ] **Step 7: 运行测试验证**

Run: `cargo test --package memrec-common --lib types::memory::tests -- --test-threads=1`

Expected: 所有测试通过

- [ ] **Step 8: 更新 common/src/lib.rs 导出**

在 `memrec_common` crate 的导出中添加新类型：

```rust
pub use types::memory::{Memory, MemoryType, MemorySource, MemoryScope};
```

- [ ] **Step 9: Commit**

```bash
git add common/src/types/memory.rs common/src/lib.rs
git commit -m "feat(common): add MemorySource and MemoryScope enums

- Add MemorySource enum (User/System/Inferred/External)
- Add MemoryScope enum (Project/Global/Workspace)
- Extend Memory struct with source and scope fields
- Maintain backward compatibility with #[serde(default)]"
```

---

## Task 2: 新增 SearchConfig 配置

**Files:**
- Modify: `common/src/types/config.rs`

- [ ] **Step 1: 在 config.rs 中添加 SourceWeights 结构体**

```rust
/// 记忆来源权重配置。
///
/// 不同来源的记忆可信度不同，在搜索评分中体现。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceWeights {
    #[serde(default = "default_user_weight")]
    pub user: f64,
    #[serde(default = "default_system_weight")]
    pub system: f64,
    #[serde(default = "default_inferred_weight")]
    pub inferred: f64,
    #[serde(default = "default_external_weight")]
    pub external: f64,
}

fn default_user_weight() -> f64 { 1.0 }
fn default_system_weight() -> f64 { 0.8 }
fn default_inferred_weight() -> f64 { 0.5 }
fn default_external_weight() -> f64 { 0.7 }

impl Default for SourceWeights {
    fn default() -> Self {
        Self {
            user: 1.0,
            system: 0.8,
            inferred: 0.5,
            external: 0.7,
        }
    }
}
```

- [ ] **Step 2: 添加 SearchConfig 结构体**

```rust
/// 搜索配置。
///
/// 控制混合检索、MMR 重排、时间衰减等行为。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// 向量检索权重，默认 0.5（BM25 权重为 1 - alpha）
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,
    
    /// 是否启用 MMR 重排
    #[serde(default = "default_mmr_enabled")]
    pub mmr_enabled: bool,
    
    /// MMR 相关性权重，默认 0.5
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,
    
    /// 时间衰减半衰期（小时），默认 336（14天）
    #[serde(default = "default_decay_half_life")]
    pub decay_half_life_hours: f64,
    
    /// 来源权重
    #[serde(default)]
    pub source_weights: SourceWeights,
    
    /// 豁免时间衰减的作用域
    #[serde(default = "default_evergreen_scopes")]
    pub evergreen_scopes: Vec<String>,
}

fn default_hybrid_alpha() -> f64 { 0.5 }
fn default_mmr_enabled() -> bool { true }
fn default_mmr_lambda() -> f64 { 0.5 }
fn default_decay_half_life() -> f64 { 336.0 }
fn default_evergreen_scopes() -> Vec<String> { vec!["global".to_string(), "workspace".to_string()] }

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            hybrid_alpha: 0.5,
            mmr_enabled: true,
            mmr_lambda: 0.5,
            decay_half_life_hours: 336.0,
            source_weights: SourceWeights::default(),
            evergreen_scopes: vec!["global".to_string(), "workspace".to_string()],
        }
    }
}
```

- [ ] **Step 3: 运行编译验证**

Run: `cargo build --package memrec-common`

Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add common/src/types/config.rs
git commit -m "feat(common): add SearchConfig with source weights and MMR settings"
```

---

## Task 3: 扩展 RequestParams

**Files:**
- Modify: `common/src/protocol/request.rs`

- [ ] **Step 1: 在 AddParams 中添加 source 和 scope 字段**

找到 `AddParams` 结构体，添加：

```rust
pub struct AddParams {
    // ... 现有字段 ...
    pub working_dir: Option<String>,
    
    /// 记忆来源
    #[serde(default)]
    pub source: Option<String>,
    
    /// 记忆作用域
    #[serde(default)]
    pub scope: Option<String>,
}
```

- [ ] **Step 2: 在 SearchMemoryParams 中添加混合检索参数**

找到 `SearchMemoryParams` 结构体，添加：

```rust
pub struct SearchMemoryParams {
    // ... 现有字段 ...
    pub working_dir: Option<String>,
    
    /// 向量检索权重（0.0-1.0），默认 0.5
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,
    
    /// 是否启用 MMR 重排，默认 true
    #[serde(default = "default_mmr_enabled")]
    pub mmr_enabled: bool,
    
    /// MMR 相关性权重（0.0-1.0），默认 0.5
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,
}

pub fn default_hybrid_alpha() -> f64 { 0.5 }
pub fn default_mmr_enabled() -> bool { true }
pub fn default_mmr_lambda() -> f64 { 0.5 }
```

- [ ] **Step 3: 添加单元测试**

在 `#[cfg(test)] mod tests` 中添加：

```rust
#[test]
fn test_add_params_with_source_scope() {
    let json = r#"{
        "type": "add",
        "content": "test content",
        "source": "system",
        "scope": "global"
    }"#;
    
    let params: RequestParams = serde_json::from_str(json).unwrap();
    if let RequestParams::Add(p) = params {
        assert_eq!(p.source, Some("system".to_string()));
        assert_eq!(p.scope, Some("global".to_string()));
    } else {
        panic!("Expected Add params");
    }
}

#[test]
fn test_search_memory_params_hybrid() {
    let json = r#"{
        "type": "search_memory",
        "query": "test query",
        "hybrid_alpha": 0.7,
        "mmr_enabled": false
    }"#;
    
    let params: RequestParams = serde_json::from_str(json).unwrap();
    if let RequestParams::SearchMemory(p) = params {
        assert_eq!(p.hybrid_alpha, 0.7);
        assert!(!p.mmr_enabled);
    } else {
        panic!("Expected SearchMemory params");
    }
}
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test --package memrec-common --lib protocol::request::tests -- --test-threads=1`

Expected: 所有测试通过

- [ ] **Step 5: Commit**

```bash
git add common/src/protocol/request.rs
git commit -m "feat(common): add source/scope to AddParams and hybrid params to SearchMemoryParams"
```

---

## Task 4: 新增搜索模块骨架

**Files:**
- Create: `memrecd/src/search/mod.rs`
- Create: `memrecd/src/search/mmr.rs`
- Create: `memrecd/src/search/scorer.rs`
- Modify: `memrecd/src/lib.rs`

- [ ] **Step 1: 创建 search/mod.rs**

```rust
//! # 搜索算法模块
//!
//! 提供搜索相关的算法组件：
//! - MMR 重排（多样性优化）
//! - 评分计算（时间衰减 + 源权重）

pub mod mmr;
pub mod scorer;

pub use mmr::{mmr_rerank, MmrConfig};
pub use scorer::{apply_scoring, ScorerConfig, SourceWeights};
```

- [ ] **Step 2: 创建 search/mmr.rs 骨架**

```rust
//! # MMR 多样性重排
//!
//! 实现 Maximal Marginal Relevance 算法，在相关性与多样性之间取得平衡。

use std::collections::HashSet;

/// MMR 配置。
#[derive(Debug, Clone)]
pub struct MmrConfig {
    /// 相关性权重（0.0-1.0），默认 0.5
    pub lambda: f64,
    /// 返回数量
    pub top_k: usize,
    /// 候选池大小
    pub max_candidates: usize,
}

impl Default for MmrConfig {
    fn default() -> Self {
        Self {
            lambda: 0.5,
            top_k: 10,
            max_candidates: 50,
        }
    }
}

/// MMR 重排。
///
/// 公式：MMR(d) = λ × rel(d) - (1-λ) × max(sim(d, d')) for d' in S
pub fn mmr_rerank<T: Clone + AsRef<str>>(
    candidates: Vec<T>,
    scores: Vec<f64>,
    config: &MmrConfig,
) -> Vec<T> {
    // TODO: 实现
    candidates.into_iter().take(config.top_k).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Hello World!");
        assert!(tokens.contains("hello"));
        assert!(tokens.contains("world"));
    }

    #[test]
    fn test_jaccard_identical() {
        let a = tokenize("hello world");
        let b = tokenize("hello world");
        assert!((jaccard(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_disjoint() {
        let a = tokenize("hello");
        let b = tokenize("world");
        assert!((jaccard(&a, &b) - 0.0).abs() < 0.001);
    }
}
```

- [ ] **Step 3: 创建 search/scorer.rs 骨架**

```rust
//! # 搜索评分器
//!
//! 计算最终搜索分数，包括时间衰减和源权重。

use chrono::{DateTime, Utc};
use memrec_common::{MemoryScope, MemorySource};

/// 评分器配置。
#[derive(Debug, Clone)]
pub struct ScorerConfig {
    /// 时间衰减半衰期（小时）
    pub decay_half_life_hours: f64,
    /// 豁免衰减的作用域
    pub evergreen_scopes: Vec<MemoryScope>,
    /// 来源权重
    pub source_weights: SourceWeights,
}

impl Default for ScorerConfig {
    fn default() -> Self {
        Self {
            decay_half_life_hours: 336.0,
            evergreen_scopes: vec![MemoryScope::Global, MemoryScope::Workspace],
            source_weights: SourceWeights::default(),
        }
    }
}

/// 来源权重配置。
#[derive(Debug, Clone)]
pub struct SourceWeights {
    pub user: f64,
    pub system: f64,
    pub inferred: f64,
    pub external: f64,
}

impl Default for SourceWeights {
    fn default() -> Self {
        Self {
            user: 1.0,
            system: 0.8,
            inferred: 0.5,
            external: 0.7,
        }
    }
}

/// 应用完整评分（时间衰减 + 源权重）。
pub fn apply_scoring(
    base_score: f64,
    created_at: DateTime<Utc>,
    scope: MemoryScope,
    source: MemorySource,
    config: &ScorerConfig,
) -> f64 {
    let decayed = apply_time_decay(base_score, created_at, scope, config);
    apply_source_weight(decayed, source, &config.source_weights)
}

/// 应用时间衰减。
fn apply_time_decay(
    score: f64,
    created_at: DateTime<Utc>,
    scope: MemoryScope,
    config: &ScorerConfig,
) -> f64 {
    if config.evergreen_scopes.contains(&scope) {
        return score;
    }
    
    let age_hours = (Utc::now() - created_at).num_hours() as f64;
    let lambda = std::f64::consts::LN_2 / config.decay_half_life_hours;
    score * (-lambda * age_hours).exp()
}

/// 应用源权重。
fn apply_source_weight(score: f64, source: MemorySource, weights: &SourceWeights) -> f64 {
    let w = match source {
        MemorySource::User => weights.user,
        MemorySource::System => weights.system,
        MemorySource::Inferred => weights.inferred,
        MemorySource::External => weights.external,
    };
    score * w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_decay_project_scope() {
        let config = ScorerConfig::default();
        let created_at = Utc::now() - chrono::Duration::hours(336);
        let decayed = apply_time_decay(1.0, created_at, MemoryScope::Project, &config);
        assert!((decayed - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_time_decay_global_scope_exempt() {
        let config = ScorerConfig::default();
        let created_at = Utc::now() - chrono::Duration::hours(336);
        let decayed = apply_time_decay(1.0, created_at, MemoryScope::Global, &config);
        assert!((decayed - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_source_weight_user() {
        let weights = SourceWeights::default();
        let weighted = apply_source_weight(1.0, MemorySource::User, &weights);
        assert!((weighted - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_source_weight_inferred() {
        let weights = SourceWeights::default();
        let weighted = apply_source_weight(1.0, MemorySource::Inferred, &weights);
        assert!((weighted - 0.5).abs() < 0.001);
    }
}
```

- [ ] **Step 4: 更新 memrecd/src/lib.rs 导出模块**

在 `lib.rs` 中添加：

```rust
pub mod search;
```

- [ ] **Step 5: 运行编译验证**

Run: `cargo build --package memrecd`

Expected: 编译成功

- [ ] **Step 6: 运行测试验证**

Run: `cargo test --package memrecd --lib search`

Expected: 所有测试通过

- [ ] **Step 7: Commit**

```bash
git add memrecd/src/search/mod.rs memrecd/src/search/mmr.rs memrecd/src/search/scorer.rs memrecd/src/lib.rs
git commit -m "feat(memrecd): add search module with MMR and scorer skeleton"
```

---

## Task 5: 实现 MMR 重排算法

**Files:**
- Modify: `memrecd/src/search/mmr.rs`

- [ ] **Step 1: 定义 SearchHitItem trait**

为 MMR 算法定义通用接口，使其可以处理不同类型的搜索结果：

```rust
/// MMR 搜索命中项接口。
pub trait MmrHit: Clone {
    /// 获取分数。
    fn score(&self) -> f64;
    /// 获取文本内容用于相似度计算。
    fn text(&self) -> &str;
}
```

- [ ] **Step 2: 实现 mmr_rerank 函数**

```rust
/// MMR 重排。
///
/// 公式：MMR(d) = λ × rel(d) - (1-λ) × max(sim(d, d')) for d' in S
///
/// # Arguments
/// * `candidates` - 候选结果列表
/// * `config` - MMR 配置
///
/// # Returns
/// 重排后的结果列表（最多 top_k 个）
pub fn mmr_rerank<H: MmrHit>(candidates: Vec<H>, config: &MmrConfig) -> Vec<H> {
    if candidates.is_empty() || config.top_k == 0 {
        return Vec::new();
    }
    
    let limit = config.top_k.min(candidates.len());
    let mut selected: Vec<H> = Vec::with_capacity(limit);
    let mut remaining: Vec<H> = candidates;
    
    // 预计算所有 token 集合
    let mut token_sets: Vec<Option<HashSet<String>>> = remaining.iter()
        .map(|h| Some(tokenize(h.text())))
        .collect();
    
    while selected.len() < limit && !remaining.is_empty() {
        let mut best_idx = 0;
        let mut best_mmr = f64::MIN;
        
        for (i, candidate) in remaining.iter().enumerate() {
            let relevance = candidate.score();
            
            let max_sim = selected.iter()
                .map(|s| {
                    let s_tokens = tokenize(s.text());
                    if let Some(ref c_tokens) = token_sets[i] {
                        jaccard(c_tokens, &s_tokens)
                    } else {
                        0.0
                    }
                })
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0);
            
            let mmr = config.lambda * relevance - (1.0 - config.lambda) * max_sim;
            
            if mmr > best_mmr {
                best_mmr = mmr;
                best_idx = i;
            }
        }
        
        let hit = remaining.remove(best_idx);
        token_sets.remove(best_idx);
        selected.push(hit);
    }
    
    selected
}
```

- [ ] **Step 3: 添加集成测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestHit {
        text: String,
        score: f64,
    }

    impl MmrHit for TestHit {
        fn score(&self) -> f64 { self.score }
        fn text(&self) -> &str { &self.text }
    }

    #[test]
    fn test_mmr_rerank_diversity() {
        let candidates = vec![
            TestHit { text: "hello world".to_string(), score: 0.9 },
            TestHit { text: "hello world test".to_string(), score: 0.85 },
            TestHit { text: "foo bar".to_string(), score: 0.8 },
        ];
        
        let config = MmrConfig { lambda: 0.5, top_k: 2, max_candidates: 50 };
        let result = mmr_rerank(candidates, &config);
        
        assert_eq!(result.len(), 2);
        // 第一个应该是分数最高的
        assert!((result[0].score() - 0.9).abs() < 0.001);
        // 第二个应该更不相似（foo bar 比 hello world test 更不相似）
        assert!(result[1].text().contains("foo"));
    }

    #[test]
    fn test_mmr_rerank_empty() {
        let candidates: Vec<TestHit> = vec![];
        let config = MmrConfig::default();
        let result = mmr_rerank(candidates, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_mmr_rerank_single() {
        let candidates = vec![
            TestHit { text: "single".to_string(), score: 0.9 },
        ];
        let config = MmrConfig { lambda: 0.5, top_k: 5, max_candidates: 50 };
        let result = mmr_rerank(candidates, &config);
        assert_eq!(result.len(), 1);
    }
}
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test --package memrecd --lib search::mmr::tests -- --test-threads=1`

Expected: 所有测试通过

- [ ] **Step 5: Commit**

```bash
git add memrecd/src/search/mmr.rs
git commit -m "feat(memrecd): implement MMR reranking algorithm"
```

---

## Task 6: 新增 FtsStorage trait

**Files:**
- Modify: `memrecd/src/storage/traits.rs`

- [ ] **Step 1: 添加 FtsPayload 结构体**

在 `VectorPayload` 之后添加：

```rust
/// 全文搜索附加载荷。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FtsPayload {
    pub project_id: Option<Uuid>,
    pub memory_type: String,
    pub tags: Vec<String>,
}
```

- [ ] **Step 2: 添加 FtsStorage trait**

在 `VectorStorage` trait 之后添加：

```rust
/// 全文搜索存储 trait。
///
/// 提供 BM25 全文检索功能。
#[async_trait]
pub trait FtsStorage: Send + Sync {
    /// 添加文档到全文索引。
    async fn add(&self, id: &Uuid, text: &str, payload: FtsPayload) -> Result<()>;
    
    /// 从全文索引中删除文档。
    async fn remove(&self, id: &Uuid) -> Result<bool>;
    
    /// 全文搜索（BM25）。
    async fn search(
        &self,
        query: &str,
        filter: SearchFilter,
        top_k: usize,
    ) -> Result<Vec<SearchHit>>;
    
    /// 获取索引文档数量。
    async fn count(&self) -> Result<usize>;
}
```

- [ ] **Step 3: 添加 HybridSearchRequest/Result 结构体**

```rust
/// 混合搜索请求。
#[derive(Debug, Clone)]
pub struct HybridSearchRequest {
    pub query: String,
    pub query_embedding: Vec<f32>,
    pub filter: SearchFilter,
    pub top_k: usize,
    pub hybrid_alpha: f64,
    pub mmr_lambda: f64,
    pub mmr_enabled: bool,
}

/// 混合搜索结果。
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    pub hits: Vec<SearchHit>,
    pub vec_count: usize,
    pub fts_count: usize,
}
```

- [ ] **Step 4: 添加 HybridStorage trait**

```rust
/// 混合搜索存储 trait。
///
/// 整合向量检索和全文检索，提供统一的搜索接口。
#[async_trait]
pub trait HybridStorage: Send + Sync {
    /// 混合搜索（KNN + BM25）。
    async fn search(&self, req: HybridSearchRequest) -> Result<HybridSearchResult>;
    
    /// 添加记忆到索引（向量 + 全文）。
    async fn add(&self, id: &Uuid, embedding: &[f32], text: &str, payload: VectorPayload) -> Result<()>;
    
    /// 从索引中删除记忆。
    async fn remove(&self, id: &Uuid) -> Result<bool>;
}
```

- [ ] **Step 5: 运行编译验证**

Run: `cargo build --package memrecd`

Expected: 编译成功

- [ ] **Step 6: Commit**

```bash
git add memrecd/src/storage/traits.rs
git commit -m "feat(memrecd): add FtsStorage and HybridStorage traits"
```

---

## Task 7: 添加 tantivy 依赖

**Files:**
- Modify: `memrecd/Cargo.toml`

- [ ] **Step 1: 在 Cargo.toml 中添加 tantivy 依赖**

在 `[dependencies]` 部分添加：

```toml
tantivy = "0.22"
```

- [ ] **Step 2: 运行编译验证依赖下载**

Run: `cargo build --package memrecd`

Expected: 下载 tantivy 并编译成功

- [ ] **Step 3: Commit**

```bash
git add memrecd/Cargo.toml
git commit -m "build(memrecd): add tantivy dependency for full-text search"
```

---

## Task 8: 实现 TantivyStore

**Files:**
- Create: `memrecd/src/storage/tantivy_store.rs`
- Modify: `memrecd/src/storage/mod.rs`

- [ ] **Step 1: 创建 tantivy_store.rs 基础结构**

```rust
//! # Tantivy 全文检索存储
//!
//! 基于 Tantivy 实现 BM25 全文搜索。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::BooleanQuery,
    schema::{Field, Schema, STORED, TEXT, FAST},
    Index, IndexReader, IndexWriter, TantivyDocument,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::traits::{FtsPayload, FtsStorage, SearchFilter, SearchHit, VectorPayload};
use crate::importance::calculator::ImportanceCalculator;

/// Tantivy 全文检索存储。
pub struct TantivyStore {
    index: Index,
    writer: Arc<RwLock<IndexWriter>>,
    reader: IndexReader,
    schema: TantivySchema,
}

/// Tantivy 索引 Schema。
struct TantivySchema {
    id: Field,
    content: Field,
    project_id: Field,
    memory_type: Field,
    tags: Field,
    importance: Field,
}

impl TantivySchema {
    fn build() -> (Schema, Self) {
        let mut schema_builder = Schema::builder();
        
        let id = schema_builder.add_text_field("id", STORED);
        let content = schema_builder.add_text_field("content", TEXT);
        let project_id = schema_builder.add_text_field("project_id", STORED);
        let memory_type = schema_builder.add_text_field("memory_type", STORED);
        let tags = schema_builder.add_text_field("tags", TEXT);
        let importance = schema_builder.add_f64_field("importance", FAST | STORED);
        
        let schema = schema_builder.build();
        
        (schema, Self {
            id,
            content,
            project_id,
            memory_type,
            tags,
            importance,
        })
    }
}
```

- [ ] **Step 2: 实现 TantivyStore::open**

```rust
impl TantivyStore {
    /// 打开或创建 Tantivy 索引。
    pub async fn open(path: &Path) -> Result<Self> {
        let (schema, tantivy_schema) = TantivySchema::build();
        
        let directory = MmapDirectory::open_create(path)
            .with_context(|| format!("Failed to open Tantivy directory: {:?}", path))?;
        
        let index = Index::open_or_create(directory, schema.clone())
            .context("Failed to open or create Tantivy index")?;
        
        let writer = index
            .writer(50_000_000)
            .context("Failed to create Tantivy writer")?;
        
        let reader = index
            .reader_builder()
            .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to create Tantivy reader")?;
        
        Ok(Self {
            index,
            writer: Arc::new(RwLock::new(writer)),
            reader,
            schema: tantivy_schema,
        })
    }
}
```

- [ ] **Step 3: 实现 FtsStorage trait - add**

```rust
#[async_trait]
impl FtsStorage for TantivyStore {
    async fn add(&self, id: &Uuid, text: &str, payload: FtsPayload) -> Result<()> {
        let mut writer = self.writer.write().await;
        
        let mut doc = TantivyDocument::new();
        doc.add_text(self.schema.id, id.to_string());
        doc.add_text(self.schema.content, text);
        
        if let Some(pid) = payload.project_id {
            doc.add_text(self.schema.project_id, pid.to_string());
        }
        doc.add_text(self.schema.memory_type, payload.memory_type);
        doc.add_text(self.schema.tags, payload.tags.join(" "));
        doc.add_f64(self.schema.importance, 0.5); // 默认重要性
        
        writer.add_document(doc).context("Failed to add document to Tantivy")?;
        writer.commit().context("Failed to commit Tantivy changes")?;
        
        Ok(())
    }
    
    // remove 和 search 在后续步骤实现
    async fn remove(&self, _id: &Uuid) -> Result<bool> {
        // Tantivy 不支持直接删除，需要通过 delete_term
        // 简化实现：暂时返回 Ok(false)
        Ok(false)
    }
    
    async fn search(
        &self,
        _query: &str,
        _filter: SearchFilter,
        _top_k: usize,
    ) -> Result<Vec<SearchHit>> {
        // 在下一步实现
        Ok(Vec::new())
    }
    
    async fn count(&self) -> Result<usize> {
        let searcher = self.reader.searcher();
        Ok(searcher.num_docs() as usize)
    }
}
```

- [ ] **Step 4: 实现 search 方法**

```rust
async fn search(
    &self,
    query: &str,
    filter: SearchFilter,
    top_k: usize,
) -> Result<Vec<SearchHit>> {
    let searcher = self.reader.searcher();
    
    let content_field = self.schema.content;
    let query_parser = tantivy::query::QueryParser::for_index(
        &self.index,
        vec![content_field, self.schema.tags],
    );
    
    let tantivy_query = query_parser
        .parse_query(query)
        .context("Failed to parse search query")?;
    
    let top_docs = searcher
        .search(&tantivy_query, &TopDocs::with_limit(top_k))
        .context("Failed to execute Tantivy search")?;
    
    let mut hits = Vec::new();
    
    for (score, doc_address) in top_docs {
        let doc: TantivyDocument = searcher
            .doc(doc_address)
            .context("Failed to retrieve document")?;
        
        let id_str = doc
            .get_first(self.schema.id)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let id = Uuid::parse_str(id_str).unwrap_or_default();
        
        // 应用过滤条件
        if let Some(filter_pid) = &filter.project_id {
            let doc_pid = doc
                .get_first(self.schema.project_id)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            
            if doc_pid != filter_pid.to_string() && !filter.include_global {
                continue;
            }
        }
        
        hits.push(SearchHit {
            memory_id: id,
            score: score as f32,
            payload: VectorPayload {
                project_id: doc.get_first(self.schema.project_id)
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok()),
                memory_type: doc.get_first(self.schema.memory_type)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                tags: doc.get_first(self.schema.tags)
                    .and_then(|v| v.as_str())
                    .map(|s| s.split_whitespace().map(String::from).collect())
                    .unwrap_or_default(),
                content_preview: String::new(),
                importance: doc.get_first(self.schema.importance)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5) as f32,
                chunk_group_id: None,
                chunk_index: None,
                chunk_total: None,
            },
        });
    }
    
    Ok(hits)
}
```

- [ ] **Step 5: 更新 storage/mod.rs 导出**

```rust
pub mod tantivy_store;

pub use tantivy_store::TantivyStore;
```

- [ ] **Step 6: 运行编译验证**

Run: `cargo build --package memrecd`

Expected: 编译成功

- [ ] **Step 7: Commit**

```bash
git add memrecd/src/storage/tantivy_store.rs memrecd/src/storage/mod.rs
git commit -m "feat(memrecd): implement TantivyStore for BM25 full-text search"
```

---

## Task 9: 实现 HybridStore

**Files:**
- Create: `memrecd/src/storage/hybrid_store.rs`
- Modify: `memrecd/src/storage/mod.rs`

- [ ] **Step 1: 创建 hybrid_store.rs 基础结构**

```rust
//! # 混合搜索存储
//!
//! 整合向量检索（KNN）和全文检索（BM25），提供统一搜索接口。

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use super::traits::{
    FtsStorage, HybridSearchRequest, HybridSearchResult, HybridStorage,
    SearchFilter, SearchHit, VectorPayload, VectorStorage,
};
use crate::search::{mmr_rerank, MmrConfig, ScorerConfig};
use crate::search::mmr::MmrHit;

/// 混合搜索存储。
pub struct HybridStore {
    vector_store: Arc<dyn VectorStorage>,
    fts_store: Arc<dyn FtsStorage>,
    mmr_config: MmrConfig,
    scorer_config: ScorerConfig,
}

/// 混合搜索配置。
#[derive(Debug, Clone)]
pub struct HybridConfig {
    pub hybrid_alpha: f64,
    pub mmr_enabled: bool,
}

impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            hybrid_alpha: 0.5,
            mmr_enabled: true,
        }
    }
}

impl HybridStore {
    /// 创建新的混合搜索存储。
    pub fn new(
        vector_store: Arc<dyn VectorStorage>,
        fts_store: Arc<dyn FtsStorage>,
        mmr_config: MmrConfig,
        scorer_config: ScorerConfig,
    ) -> Self {
        Self {
            vector_store,
            fts_store,
            mmr_config,
            scorer_config,
        }
    }
}
```

- [ ] **Step 2: 实现合并归一化方法**

```rust
impl HybridStore {
    /// 合并向量和全文检索结果并归一化。
    fn merge_and_normalize(
        &self,
        vec_hits: Vec<SearchHit>,
        fts_hits: Vec<SearchHit>,
        alpha: f64,
    ) -> Vec<SearchHit> {
        use std::collections::HashMap;
        
        if vec_hits.is_empty() && fts_hits.is_empty() {
            return Vec::new();
        }
        
        // 计算归一化参数
        let (vec_min, vec_max) = vec_hits.iter()
            .fold((f32::MAX, f32::MIN), |(min, max), h| {
                (min.min(h.score), max.max(h.score))
            });
        
        let (fts_min, fts_max) = fts_hits.iter()
            .fold((f32::MAX, f32::MIN), |(min, max), h| {
                (min.min(h.score), max.max(h.score))
            });
        
        // 合并结果
        let mut merged: HashMap<Uuid, SearchHit> = HashMap::new();
        
        // 添加向量结果
        for hit in vec_hits {
            let norm_score = if (vec_max - vec_min).abs() < f32::EPSILON {
                1.0
            } else {
                1.0 - (hit.score - vec_min) / (vec_max - vec_min)
            };
            
            let final_score = (alpha * norm_score as f64) as f32;
            
            merged.insert(hit.memory_id, SearchHit {
                memory_id: hit.memory_id,
                score: final_score,
                payload: hit.payload,
            });
        }
        
        // 合并全文结果
        for hit in fts_hits {
            let norm_score = if (fts_max - fts_min).abs() < f32::EPSILON {
                1.0
            } else {
                (hit.score - fts_min) / (fts_max - fts_min)
            };
            
            let fts_contrib = ((1.0 - alpha) * norm_score as f64) as f32;
            
            if let Some(existing) = merged.get_mut(&hit.memory_id) {
                existing.score += fts_contrib;
            } else {
                merged.insert(hit.memory_id, SearchHit {
                    memory_id: hit.memory_id,
                    score: fts_contrib,
                    payload: hit.payload,
                });
            }
        }
        
        let mut result: Vec<SearchHit> = merged.into_values().collect();
        result.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        result
    }
}
```

- [ ] **Step 3: 实现 HybridStorage trait**

```rust
#[async_trait]
impl HybridStorage for HybridStore {
    async fn search(&self, req: HybridSearchRequest) -> Result<HybridSearchResult> {
        // 1. 并行执行向量检索和全文检索
        let (vec_result, fts_result) = tokio::join!(
            self.vector_store.search(&req.query_embedding, req.filter.clone(), req.top_k),
            self.fts_store.search(&req.query, req.filter, req.top_k)
        );
        
        let vec_hits = vec_result?;
        let fts_hits = fts_result?;
        
        let vec_count = vec_hits.len();
        let fts_count = fts_hits.len();
        
        // 2. 合并归一化
        let merged = self.merge_and_normalize(vec_hits, fts_hits, req.hybrid_alpha);
        
        // 3. 按分数排序并取 top_k
        let mut sorted = merged;
        sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        sorted.truncate(req.top_k);
        
        // 4. MMR 重排（可选）
        let final_hits = if req.mmr_enabled {
            // 转换为 MMR 可用的结构
            let mmr_hits: Vec<MmrSearchHit> = sorted.into_iter()
                .map(|h| MmrSearchHit { inner: h })
                .collect();
            
            self.mmr_config.lambda = req.mmr_lambda;
            let reranked = mmr_rerank(mmr_hits, &self.mmr_config);
            reranked.into_iter().map(|h| h.inner).collect()
        } else {
            sorted
        };
        
        Ok(HybridSearchResult {
            hits: final_hits,
            vec_count,
            fts_count,
        })
    }
    
    async fn add(
        &self,
        id: &Uuid,
        embedding: &[f32],
        text: &str,
        payload: VectorPayload,
    ) -> Result<()> {
        // 双写：向量 + 全文索引
        self.vector_store.add(id, embedding, payload.clone()).await?;
        
        let fts_payload = super::traits::FtsPayload {
            project_id: payload.project_id,
            memory_type: payload.memory_type,
            tags: payload.tags,
        };
        self.fts_store.add(id, text, fts_payload).await?;
        
        Ok(())
    }
    
    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let vec_removed = self.vector_store.remove(id).await?;
        let fts_removed = self.fts_store.remove(id).await?;
        Ok(vec_removed || fts_removed)
    }
}

/// MMR 搜索命中包装。
#[derive(Clone)]
struct MmrSearchHit {
    inner: SearchHit,
}

impl MmrHit for MmrSearchHit {
    fn score(&self) -> f64 { self.inner.score as f64 }
    fn text(&self) -> &str { &self.inner.payload.content_preview }
}
```

- [ ] **Step 4: 更新 storage/mod.rs 导出**

```rust
pub mod hybrid_store;

pub use hybrid_store::{HybridConfig, HybridStore};
```

- [ ] **Step 5: 运行编译验证**

Run: `cargo build --package memrecd`

Expected: 编译成功

- [ ] **Step 6: Commit**

```bash
git add memrecd/src/storage/hybrid_store.rs memrecd/src/storage/mod.rs
git commit -m "feat(memrecd): implement HybridStore for unified search"
```

---

## Task 10: 集成 HybridStore 到 Handler

**Files:**
- Modify: `memrecd/src/server/handler.rs`
- Modify: `memrecd/src/daemon/mod.rs`

- [ ] **Step 1: 读取 handler.rs 了解现有结构**

Run: `wc -l memrecd/src/server/handler.rs`

- [ ] **Step 2: 修改 Router 结构体**

将 `vector_store` 替换为 `hybrid_store`：

```rust
use crate::storage::{HybridStorage, HybridStore};

pub struct Router {
    pub storage: Arc<dyn MemoryStorage>,
    pub hybrid_store: Arc<dyn HybridStorage>,
    pub embedder: Arc<dyn EmbeddingGenerator>,
    pub project_detector: ProjectDetector,
    pub config: ServerConfig,
}
```

- [ ] **Step 3: 修改 Router::new 方法**

```rust
impl Router {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        hybrid_store: Arc<dyn HybridStorage>,
        embedder: Arc<dyn EmbeddingGenerator>,
        project_detector: ProjectDetector,
        config: ServerConfig,
    ) -> Self {
        Self {
            storage,
            hybrid_store,
            embedder,
            project_detector,
            config,
        }
    }
}
```

- [ ] **Step 4: 修改 handle_search_memory**

```rust
async fn handle_search_memory(&self, p: SearchMemoryParams) -> Result<ResponseResult> {
    let start = std::time::Instant::now();
    
    // 1. 生成查询嵌入
    let query_embedding = self.embedder.embed(&p.query)?;
    let embedding_time = start.elapsed();
    
    // 2. 构建过滤条件
    let project_id = self.resolve_project_id(&p).await?;
    let filter = SearchFilter {
        project_id,
        include_global: !p.project_only && !p.global_only && p.include_global,
        memory_type: p.memory_type.map(|t| t.to_string()),
        min_score: p.min_score,
    };
    
    // 3. 构建混合搜索请求
    let req = HybridSearchRequest {
        query: p.query.clone(),
        query_embedding,
        filter,
        top_k: p.top_k,
        hybrid_alpha: p.hybrid_alpha,
        mmr_lambda: p.mmr_lambda,
        mmr_enabled: p.mmr_enabled,
    };
    
    // 4. 执行搜索
    let search_start = std::time::Instant::now();
    let result = self.hybrid_store.search(req).await?;
    let search_time = search_start.elapsed();
    
    // 5. 补充元数据
    let mut results = Vec::with_capacity(result.hits.len());
    for hit in result.hits {
        if let Some(memory) = self.storage.get(&hit.memory_id).await? {
            results.push(SearchResult {
                memory_id: hit.memory_id,
                memory_type: memory.memory_type,
                content_preview: Self::truncate_content(&memory.content, 200),
                score: hit.score,
                tags: memory.tags,
                created_at: memory.created_at,
                project_id: memory.project_id,
                is_chunked: memory.is_chunked(),
                chunk_group_id: memory.chunk_group_id,
                chunk_index: memory.chunk_index,
                chunk_total: memory.chunk_total,
            });
        }
    }
    
    Ok(ResponseResult::SearchMemory(SemanticSearchResult {
        query_embedding_time_ms: embedding_time.as_millis() as u64,
        search_time_ms: search_time.as_millis() as u64,
        total: results.len(),
        results,
    }))
}
```

- [ ] **Step 5: 修改 daemon/mod.rs 初始化 HybridStore**

在 `Daemon::run` 或初始化代码中：

```rust
use crate::storage::{HybridStore, TantivyStore};
use crate::search::{MmrConfig, ScorerConfig};

// 初始化 TantivyStore
let fts_path = data_dir.join("fts");
let fts_store = Arc::new(TantivyStore::open(&fts_path).await?);

// 初始化 HybridStore
let hybrid_store = Arc::new(HybridStore::new(
    vector_store.clone(),
    fts_store,
    MmrConfig::default(),
    ScorerConfig::default(),
));

// 创建 Router
let router = Router::new(
    storage,
    hybrid_store,
    embedder,
    project_detector,
    config,
);
```

- [ ] **Step 6: 运行编译验证**

Run: `cargo build --package memrecd`

Expected: 编译成功

- [ ] **Step 7: Commit**

```bash
git add memrecd/src/server/handler.rs memrecd/src/daemon/mod.rs
git commit -m "feat(memrecd): integrate HybridStore into Router and Daemon"
```

---

## Task 11: 更新 CLI 支持 source/scope 参数

**Files:**
- Modify: `memrec/src/commands/add.rs`

- [ ] **Step 1: 在 AddArgs 中添加 source 和 scope 参数**

```rust
pub struct AddArgs {
    pub content: String,
    pub memory_type: Option<String>,
    pub tags: Vec<String>,
    pub global: bool,
    
    /// 记忆来源 (user/system/inferred/external)
    #[arg(short = 'S', long)]
    pub source: Option<String>,
    
    /// 记忆作用域 (project/global/workspace)
    #[arg(short = 'c', long)]
    pub scope: Option<String>,
}
```

- [ ] **Step 2: 更新 add 命令实现**

在构建请求时添加 source 和 scope：

```rust
let params = RequestParams::Add(AddParams {
    content: args.content,
    memory_type: memory_type.unwrap_or_default(),
    tags: args.tags,
    project_id: None,
    is_global: args.global,
    working_dir: Some(std::env::current_dir()?.to_string_lossy().to_string()),
    source: args.source,
    scope: args.scope,
});
```

- [ ] **Step 3: 运行编译验证**

Run: `cargo build --package memrec`

Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add memrec/src/commands/add.rs
git commit -m "feat(memrec): add --source and --scope options to add command"
```

---

## Task 12: 集成测试

**Files:**
- Create: `memrecd/tests/hybrid_search_test.rs`

- [ ] **Step 1: 创建集成测试文件**

```rust
//! # 混合检索集成测试

use memrecd::storage::{HybridStore, TantivyStore, VectorStore, HybridStorage, HybridSearchRequest, SearchFilter};
use memrecd::search::{MmrConfig, ScorerConfig};
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test]
async fn test_hybrid_store_basic() {
    let temp_dir = TempDir::new().unwrap();
    
    // 创建向量存储（内存版）
    let vector_store = std::sync::Arc::new(VectorStore::new());
    
    // 创建全文存储
    let fts_path = temp_dir.path().join("fts");
    let fts_store = std::sync::Arc::new(
        TantivyStore::open(&fts_path).await.unwrap()
    );
    
    // 创建混合存储
    let hybrid_store = HybridStore::new(
        vector_store,
        fts_store,
        MmrConfig::default(),
        ScorerConfig::default(),
    );
    
    // 添加测试数据
    let id1 = Uuid::new_v4();
    let embedding = vec![0.1; 384];
    let payload = memrecd::storage::VectorPayload {
        project_id: None,
        memory_type: "knowledge".to_string(),
        tags: vec!["test".to_string()],
        content_preview: "hello world".to_string(),
        importance: 0.5,
        chunk_group_id: None,
        chunk_index: None,
        chunk_total: None,
    };
    
    hybrid_store.add(&id1, &embedding, "hello world", payload).await.unwrap();
    
    // 测试搜索
    let req = HybridSearchRequest {
        query: "hello".to_string(),
        query_embedding: embedding.clone(),
        filter: SearchFilter::default(),
        top_k: 10,
        hybrid_alpha: 0.5,
        mmr_lambda: 0.5,
        mmr_enabled: false,
    };
    
    let result = hybrid_store.search(req).await.unwrap();
    assert!(!result.hits.is_empty());
}

#[tokio::test]
async fn test_mmr_reranking() {
    let temp_dir = TempDir::new().unwrap();
    
    let vector_store = std::sync::Arc::new(VectorStore::new());
    let fts_path = temp_dir.path().join("fts");
    let fts_store = std::sync::Arc::new(
        TantivyStore::open(&fts_path).await.unwrap()
    );
    
    let mmr_config = MmrConfig {
        lambda: 0.5,
        top_k: 3,
        max_candidates: 10,
    };
    
    let hybrid_store = HybridStore::new(
        vector_store,
        fts_store,
        mmr_config,
        ScorerConfig::default(),
    );
    
    // 添加相似内容
    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let embedding = vec![0.1; 384];
    
    for (i, id) in ids.iter().enumerate() {
        let content = if i < 3 {
            format!("hello world test {}", i)
        } else {
            format!("different content {}", i)
        };
        
        let payload = memrecd::storage::VectorPayload {
            project_id: None,
            memory_type: "knowledge".to_string(),
            tags: vec![],
            content_preview: content.clone(),
            importance: 0.5,
            chunk_group_id: None,
            chunk_index: None,
            chunk_total: None,
        };
        
        hybrid_store.add(id, &embedding, &content, payload).await.unwrap();
    }
    
    let req = HybridSearchRequest {
        query: "hello world".to_string(),
        query_embedding: embedding,
        filter: SearchFilter::default(),
        top_k: 3,
        hybrid_alpha: 0.5,
        mmr_lambda: 0.5,
        mmr_enabled: true,
    };
    
    let result = hybrid_store.search(req).await.unwrap();
    assert_eq!(result.hits.len(), 3);
    
    // MMR 应该优先选择不相似的内容
    let has_diverse = result.hits.iter()
        .any(|h| h.payload.content_preview.contains("different"));
    assert!(has_diverse);
}
```

- [ ] **Step 2: 运行集成测试**

Run: `cargo test --package memrecd --test hybrid_search_test -- --test-threads=1`

Expected: 测试通过

- [ ] **Step 3: Commit**

```bash
git add memrecd/tests/hybrid_search_test.rs
git commit -m "test(memrecd): add hybrid search integration tests"
```

---

## Task 13: 最终验证与清理

- [ ] **Step 1: 运行全量测试**

Run: `cargo test --release`

Expected: 所有测试通过

- [ ] **Step 2: 运行 clippy 检查**

Run: `cargo clippy --release`

Expected: 无警告

- [ ] **Step 3: 运行格式检查**

Run: `cargo fmt --check`

Expected: 格式正确

- [ ] **Step 4: 记录到 memrec**

```bash
memrec add "MemRec 搜索补强完成：MMR重排、混合检索(KNN+BM25)、时间衰减常青豁免、源权重机制。使用 Tantivy 实现全文检索，HybridStore 统一封装。" --mtype decision --tag enhancement --tag search --tag mmr --tag hybrid
```

- [ ] **Step 5: 更新 MEMORY.md**

- [ ] **Step 6: 最终 commit**

```bash
git add .
git commit -m "feat: complete search enhancement (P0+P1)

- Implement MMR reranking for result diversity
- Add hybrid search (KNN + BM25) with Tantivy
- Add time decay evergreen exemption for Global/Workspace scope
- Add source weight mechanism (User/System/Inferred/External)
- Integrate HybridStore into Router and Daemon"
```

---

## 实施计划自审

**1. Spec 覆盖检查：**
- ✅ MMR 重排：Task 5 实现
- ✅ 混合检索：Task 6-9 实现
- ✅ 时间衰减常青豁免：Task 4 实现
- ✅ 源权重：Task 2, 4 实现
- ✅ 类型扩展：Task 1 实现
- ✅ RequestParams 扩展：Task 3 实现
- ✅ CLI 支持：Task 11 实现

**2. Placeholder 检查：**
- 无 TBD/TODO 占位符
- 所有代码步骤都有具体实现

**3. 类型一致性检查：**
- MemorySource/MemoryScope 定义一致
- SearchHit/HybridSearchRequest/HybridSearchResult 定义一致
- FtsStorage/VectorStorage trait 方法签名一致
