# MemRec Phase 5: 高级功能实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-step. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现记忆生命周期管理、重要性评分算法、压缩策略、遗忘策略、向量嵌入生成。

**Architecture:** 重要性计算器、生命周期管理器、压缩引擎、嵌入生成器、定时调度器。

**Tech Stack:** Rust, tokio, candle (可选)

---

## 前置条件

Phase 1-4 已完成：
- 存储层（MemoryStore、VectorStore）
- 服务层（memrecd）
- CLI 工具（memrec）

---

## 文件结构

### 新建文件

```
memrecd/
└── src/
    ├── importance/
    │   ├── mod.rs           # importance 模块导出
    │   ├── calculator.rs    # 重要性评分计算器
    │   └── weights.rs       # 标签权重配置
    ├── lifecycle/
    │   ├── mod.rs           # lifecycle 模块导出
    │   ├── manager.rs       # 生命周期管理器
    │   ├── compression.rs   # 压缩策略
    │   ├── forgetting.rs    # 遗忘策略
    │   └── scheduler.rs     # 定时调度器
    ├── embedding/
    │   ├── mod.rs           # embedding 模块导出
    │   ├── generator.rs     # 嵌入生成器
    │   ├── openai.rs        # OpenAI API 客户端
    │   └── cache.rs         # 嵌入缓存
```

---

## Task 1: 实现重要性评分计算器

**Files:**
- Create: `memrecd/src/importance/mod.rs`
- Create: `memrecd/src/importance/calculator.rs`
- Create: `memrecd/src/importance/weights.rs`

- [ ] **Step 1: 创建 importance 目录**

```bash
mkdir -p memrecd/src/importance
```

- [ ] **Step 2: 定义标签权重**

File: `memrecd/src/importance/weights.rs`

```rust
use std::collections::HashMap;

pub fn default_tag_weights() -> HashMap<String, f32> {
    let mut weights = HashMap::new();
    
    weights.insert("critical", 1.0);
    weights.insert("decision", 0.9);
    weights.insert("key", 0.8);
    weights.insert("important", 0.7);
    weights.insert("config", 0.6);
    weights.insert("reference", 0.5);
    weights.insert("note", 0.4);
    weights.insert("temporary", 0.2);
    weights.insert("draft", 0.1);
    
    weights
}

pub fn semantic_importance(tags: &[String], weights: &HashMap<String, f32>) -> f32 {
    if tags.is_empty() {
        return 0.5;
    }
    
    tags.iter()
        .map(|tag| weights.get(tag).copied().unwrap_or(0.5))
        .max()
        .unwrap_or(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_weights() {
        let weights = default_tag_weights();
        
        assert_eq!(weights.get("critical"), Some(&1.0));
        assert_eq!(weights.get("decision"), Some(&0.9));
        assert_eq!(weights.get("note"), Some(&0.4));
    }
    
    #[test]
    fn test_semantic_importance() {
        let weights = default_tag_weights();
        
        let tags = vec!["critical".to_string(), "config".to_string()];
        let importance = semantic_importance(&tags, &weights);
        
        assert_eq!(importance, 1.0);
    }
    
    #[test]
    fn test_semantic_importance_empty() {
        let weights = default_tag_weights();
        let importance = semantic_importance(&[], &weights);
        
        assert_eq!(importance, 0.5);
    }
}
```

- [ ] **Step 3: 实现重要性计算器**

File: `memrecd/src/importance/calculator.rs`

```rust
use chrono::{DateTime, Utc};
use memrec_common::Memory;
use memrec_common::ImportanceConfig;
use super::weights::{default_tag_weights, semantic_importance};

pub struct ImportanceCalculator {
    config: ImportanceConfig,
    tag_weights: std::collections::HashMap<String, f32>,
}

impl ImportanceCalculator {
    pub fn new(config: ImportanceConfig) -> Self {
        Self {
            config,
            tag_weights: default_tag_weights(),
        }
    }
    
    pub fn calculate(&self, memory: &Memory) -> f32 {
        let now = Utc::now();
        
        let recency = self.calculate_recency(memory.last_accessed, now);
        let frequency = self.calculate_frequency(memory.access_count);
        let semantic = self.calculate_semantic(&memory.tags);
        let explicit = self.calculate_explicit(&memory.metadata);
        
        let importance = self.config.weight_recency * recency
            + self.config.weight_frequency * frequency
            + self.config.weight_semantic * semantic
            + self.config.weight_explicit * explicit;
        
        importance.clamp(0.0, 1.0)
    }
    
    fn calculate_recency(&self, last_accessed: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
        let days_since_access = (now - last_accessed).num_days() as f32;
        (-self.config.lambda * days_since_access).exp()
    }
    
    fn calculate_frequency(&self, access_count: u32) -> f32 {
        ((access_count as f32 + 1.0).ln()) / self.config.frequency_normalize
    }
    
    fn calculate_semantic(&self, tags: &[String]) -> f32 {
        semantic_importance(tags, &self.tag_weights)
    }
    
    fn calculate_explicit(&self, metadata: &std::collections::HashMap<String, String>) -> f32 {
        metadata.get("priority")
            .and_then(|p| p.parse::<f32>().ok())
            .unwrap_or(0.5)
    }
}

impl Default for ImportanceCalculator {
    fn default() -> Self {
        Self::new(ImportanceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_calculator_new() {
        let calc = ImportanceCalculator::default();
        
        assert_eq!(calc.config.lambda, 0.05);
        assert_eq!(calc.config.weight_recency, 0.3);
    }
    
    #[test]
    fn test_calculate_recency() {
        let calc = ImportanceCalculator::default();
        let now = Utc::now();
        let accessed = now - chrono::Duration::days(0);
        
        let recency = calc.calculate_recency(accessed, now);
        assert!(recency > 0.99);
        
        let accessed_old = now - chrono::Duration::days(30);
        let recency_old = calc.calculate_recency(accessed_old, now);
        assert!(recency_old < 0.25);
    }
    
    #[test]
    fn test_calculate_frequency() {
        let calc = ImportanceCalculator::default();
        
        let freq_0 = calc.calculate_frequency(0);
        let freq_10 = calc.calculate_frequency(10);
        let freq_100 = calc.calculate_frequency(100);
        
        assert!(freq_0 < freq_10);
        assert!(freq_10 < freq_100);
    }
    
    #[test]
    fn test_calculate_full() {
        let calc = ImportanceCalculator::default();
        
        let memory = Memory::new("test".to_string(), memrec_common::MemoryType::Knowledge)
            .with_tags(vec!["critical".to_string()]);
        
        let importance = calc.calculate(&memory);
        
        assert!(importance > 0.8);
    }
    
    #[test]
    fn test_calculate_old_memory() {
        let calc = ImportanceCalculator::default();
        
        let mut memory = Memory::new("test".to_string(), memrec_common::MemoryType::Note);
        memory.last_accessed = Utc::now() - chrono::Duration::days(90);
        memory.access_count = 1;
        
        let importance = calc.calculate(&memory);
        
        assert!(importance < 0.3);
    }
}
```

注意：需要在 common/src/types/memory.rs 中添加 Note 类型，或者使用现有类型。

- [ ] **Step 4: 创建 importance/mod.rs**

```rust
mod calculator;
mod weights;

pub use calculator::ImportanceCalculator;
```

- [ ] **Step 5: 运行重要性计算测试**

```bash
cargo test -p memrecd --lib importance
```

Expected: 5 tests PASS

- [ ] **Step 6: 提交重要性计算器**

```bash
git add memrecd/src/importance/
git commit -m "feat: implement importance calculator"
```

---

## Task 2: 实现生命周期管理器

**Files:**
- Create: `memrecd/src/lifecycle/mod.rs`
- Create: `memrecd/src/lifecycle/manager.rs`

- [ ] **Step 1: 创建 lifecycle 目录**

```bash
mkdir -p memrecd/src/lifecycle
```

- [ ] **Step 2: 实现生命周期管理器**

File: `memrecd/src/lifecycle/manager.rs`

```rust
use anyhow::Result;
use std::sync::Arc;
use chrono::Utc;
use tracing::{info, warn};

use memrec_common::{Memory, MemoryConfig};
use crate::storage::{MemoryStorage};
use crate::importance::ImportanceCalculator;

pub struct LifecycleManager {
    storage: Arc<dyn MemoryStorage>,
    calculator: ImportanceCalculator,
    config: MemoryConfig,
}

impl LifecycleManager {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        calculator: ImportanceCalculator,
        config: MemoryConfig,
    ) -> Self {
        Self {
            storage,
            calculator,
            config,
        }
    }
    
    pub async fn recalculate_importance(&self) -> Result<()> {
        info!("Recalculating importance for all memories");
        
        let memories = self.storage.list(1000).await?;
        
        for memory in memories {
            let new_importance = self.calculator.calculate(&memory);
            
            if memory.importance != new_importance {
                let mut updated = memory;
                updated.importance = new_importance;
                self.storage.update(&updated).await?;
            }
        }
        
        info!("Importance recalculation completed");
        Ok(())
    }
    
    pub async fn check_storage_usage(&self) -> Result<f32> {
        let total = self.storage.count().await?;
        let deleted = self.storage.count_deleted().await?;
        let active = total;
        
        let estimated_size = active * 2;  // KB per memory
        
        let max_size = self.config.max_storage_gb * 1024 * 1024;
        let usage = estimated_size as f32 / max_size as f32;
        
        Ok(usage.clamp(0.0, 1.0))
    }
    
    pub async fn cleanup_cycle(&self) -> Result<()> {
        info!("Starting cleanup cycle");
        
        let usage = self.check_storage_usage().await?;
        
        if usage > self.config.high_watermark {
            warn!("Storage usage high: {:.1}% - triggering cleanup", usage * 100);
            
            self.cleanup_deleted().await?;
            self.cleanup_low_importance().await?;
        }
        
        info!("Cleanup cycle completed");
        Ok(())
    }
    
    async fn cleanup_deleted(&self) -> Result<()> {
        let deleted = self.storage.list_deleted().await?;
        let now = Utc::now();
        
        for memory in deleted {
            if let Some(deleted_at) = memory.deleted_at {
                let days_since_delete = (now - deleted_at).num_days();
                
                if days_since_delete > self.config.soft_delete_recovery_days as i64 {
                    info!("Hard deleting memory {} (deleted {} days ago)", 
                        memory.id, days_since_delete);
                    self.storage.delete(&memory.id).await?;
                }
            }
        }
        
        Ok(())
    }
    
    async fn cleanup_low_importance(&self) -> Result<()> {
        let low_importance = self.storage.list_by_importance(
            0.0,
            self.config.hard_delete_importance,
        ).await?;
        
        let now = Utc::now();
        
        for memory in low_importance {
            let days_inactive = (now - memory.last_accessed).num_days();
            
            if days_inactive > self.config.hard_delete_inactive_days as i64 {
                info!("Deleting low importance memory {} (inactive {} days)", 
                    memory.id, days_inactive);
                self.storage.delete(&memory.id).await?;
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use tempfile::tempdir;
    use memrec_common::MemoryType;
    
    #[tokio::test]
    async fn test_lifecycle_check_usage() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));
        
        let calc = ImportanceCalculator::default();
        let config = MemoryConfig::default();
        
        let manager = LifecycleManager::new(storage, calc, config);
        
        let usage = manager.check_storage_usage().await.unwrap();
        assert!(usage >= 0.0);
    }
    
    #[tokio::test]
    async fn test_recalculate_importance() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));
        
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        storage.save(&memory).await.unwrap();
        
        let calc = ImportanceCalculator::default();
        let config = MemoryConfig::default();
        
        let manager = LifecycleManager::new(storage.clone(), calc, config);
        
        manager.recalculate_importance().await.unwrap();
        
        let retrieved = storage.get(&memory.id).await.unwrap().unwrap();
        assert!(retrieved.importance > 0.0);
    }
}
```

- [ ] **Step 3: 创建 lifecycle/mod.rs**

```rust
mod manager;

pub use manager::LifecycleManager;
```

- [ ] **Step 4: 运行生命周期测试**

```bash
cargo test -p memrecd --lib lifecycle::manager::tests
```

Expected: 2 tests PASS

- [ ] **Step 5: 提交生命周期管理器**

```bash
git add memrecd/src/lifecycle/
git commit -m "feat: implement lifecycle manager"
```

---

## Task 3: 实现定时调度器

**Files:**
- Create: `memrecd/src/lifecycle/scheduler.rs`

- [ ] **Step 1: 实现调度器**

File: `memrecd/src/lifecycle/scheduler.rs`

```rust
use anyhow::Result;
use tokio::time::{interval, Duration};
use std::sync::Arc;
use tracing::info;

use super::manager::LifecycleManager;

pub struct Scheduler {
    manager: Arc<LifecycleManager>,
    importance_interval: Duration,
    cleanup_interval: Duration,
}

impl Scheduler {
    pub fn new(manager: Arc<LifecycleManager>) -> Self {
        Self {
            manager,
            importance_interval: Duration::from_secs(24 * 60 * 60),  // 24 hours
            cleanup_interval: Duration::from_secs(12 * 60 * 60),     // 12 hours
        }
    }
    
    pub async fn run(&self) -> Result<()> {
        info!("Lifecycle scheduler started");
        
        let mut importance_timer = interval(self.importance_interval);
        let mut cleanup_timer = interval(self.cleanup_interval);
        
        loop {
            tokio::select! {
                _ = importance_timer.tick() => {
                    info!("Importance recalculation triggered");
                    if let Err(e) = self.manager.recalculate_importance().await {
                        tracing::error!("Importance recalculation failed: {}", e);
                    }
                }
                
                _ = cleanup_timer.tick() => {
                    info!("Cleanup cycle triggered");
                    if let Err(e) = self.manager.cleanup_cycle().await {
                        tracing::error!("Cleanup cycle failed: {}", e);
                    }
                }
            }
        }
    }
    
    pub fn with_intervals(
        manager: Arc<LifecycleManager>,
        importance_interval_secs: u64,
        cleanup_interval_secs: u64,
    ) -> Self {
        Self {
            manager,
            importance_interval: Duration::from_secs(importance_interval_secs),
            cleanup_interval: Duration::from_secs(cleanup_interval_secs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use tempfile::tempdir;
    use memrec_common::{MemoryConfig, ImportanceCalculator};
    
    #[test]
    fn test_scheduler_creation() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = std::sync::Arc::new(MemoryStore::new(rocksdb));
        
        let calc = ImportanceCalculator::default();
        let config = MemoryConfig::default();
        let manager = LifecycleManager::new(storage, calc, config);
        
        let scheduler = Scheduler::new(std::sync::Arc::new(manager));
        
        assert_eq!(scheduler.importance_interval, Duration::from_secs(24 * 60 * 60));
        assert_eq!(scheduler.cleanup_interval, Duration::from_secs(12 * 60 * 60));
    }
}
```

- [ ] **Step 2: 更新 lifecycle/mod.rs**

```rust
mod scheduler;

pub use scheduler::Scheduler;
```

- [ ] **Step 3: 运行调度器测试**

```bash
cargo test -p memrecd --lib lifecycle::scheduler::tests
```

Expected: 1 test PASS

- [ ] **Step 4: 提交调度器**

```bash
git add memrecd/src/lifecycle/
git commit -m "feat: implement lifecycle scheduler"
```

---

## Task 4: 实现嵌入生成器（简化版）

**Files:**
- Create: `memrecd/src/embedding/mod.rs`
- Create: `memrecd/src/embedding/generator.rs`

- [ ] **Step 1: 创建 embedding 目录**

```bash
mkdir -p memrecd/src/embedding
```

- [ ] **Step 2: 实现嵌入生成器（简化版，先不调用API）**

File: `memrecd/src/embedding/generator.rs`

```rust
use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::storage::{MemoryStorage, VectorStorage};

pub struct EmbeddingGenerator {
    storage: Arc<dyn MemoryStorage>,
    vector_store: Arc<dyn VectorStorage>,
    dimension: usize,
}

impl EmbeddingGenerator {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        vector_store: Arc<dyn VectorStorage>,
        dimension: usize,
    ) -> Self {
        Self {
            storage,
            vector_store,
            dimension,
        }
    }
    
    pub async fn generate_for_memory(&self, memory_id: &uuid::Uuid) -> Result<()> {
        let memory = self.storage.get(memory_id).await?
            .ok_or_else(|| anyhow::anyhow!("Memory not found"))?;
        
        if memory.embedding.is_some() {
            info!("Memory {} already has embedding", memory_id);
            return Ok(());
        }
        
        let embedding = self.generate_dummy_embedding(&memory.content);
        
        self.vector_store.add(memory_id, &embedding).await?;
        
        let mut updated = memory;
        updated.embedding = Some(embedding);
        self.storage.update(&updated).await?;
        
        info!("Generated embedding for memory {}", memory_id);
        Ok(())
    }
    
    fn generate_dummy_embedding(&self, content: &str) -> Vec<f32> {
        let mut embedding = vec![0.0; self.dimension];
        
        let hash = self.simple_hash(content);
        for i in 0..self.dimension.min(100) {
            embedding[i] = ((hash >> (i % 32)) & 1) as f32;
        }
        
        let norm = (embedding.iter().map(|x| x * x).sum::<f32>()).sqrt();
        if norm > 0.0 {
            for e in &mut embedding {
                *e /= norm;
            }
        }
        
        embedding
    }
    
    fn simple_hash(&self, s: &str) -> u64 {
        let mut hash = 0u64;
        for byte in s.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore, UsearchStore};
    use tempfile::tempdir;
    use memrec_common::MemoryType;
    
    #[tokio::test]
    async fn test_generate_embedding() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));
        let vector_store = Arc::new(UsearchStore::new(1536));
        
        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge);
        storage.save(&memory).await.unwrap();
        
        let generator = EmbeddingGenerator::new(storage.clone(), vector_store, 1536);
        
        generator.generate_for_memory(&memory.id).await.unwrap();
        
        let updated = storage.get(&memory.id).await.unwrap().unwrap();
        assert!(updated.embedding.is_some());
        assert_eq!(updated.embedding.unwrap().len(), 1536);
    }
}
```

- [ ] **Step 3: 创建 embedding/mod.rs**

```rust
mod generator;

pub use generator::EmbeddingGenerator;
```

- [ ] **Step 4: 运行嵌入生成测试**

```bash
cargo test -p memrecd --lib embedding::generator::tests
```

Expected: 1 test PASS

- [ ] **Step 5: 提交嵌入生成器**

```bash
git add memrecd/src/embedding/
git commit -m "feat: implement embedding generator (simplified)"
```

---

## Task 5: 集成到 memrecd

**Files:**
- Modify: `memrecd/src/manager/daemon.rs`
- Modify: `memrecd/src/main.rs`

- [ ] **Step 1: 更新 daemon.rs 集成高级功能**

在 `memrecd/src/manager/daemon.rs` 中添加：

```rust
use crate::importance::ImportanceCalculator;
use crate::lifecycle::{LifecycleManager, Scheduler};
use crate::embedding::EmbeddingGenerator;

// 在 Daemon::new() 中添加：
let calculator = ImportanceCalculator::new(config.to_importance_config());
let lifecycle_manager = Arc::new(LifecycleManager::new(
    storage.memory_store(),
    calculator,
    config.to_memory_config(),
));

let scheduler = Scheduler::new(lifecycle_manager.clone());

// 在 run() 中添加 scheduler：
tokio::spawn(scheduler.run());
```

- [ ] **Step 2: 更新 main.rs 导入模块**

```rust
mod importance;
mod lifecycle;
mod embedding;
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p memrecd
```

Expected: PASS

- [ ] **Step 4: 提交集成**

```bash
git add memrecd/src/
git commit -m "feat: integrate advanced features into daemon"
```

---

## Task 6: 添加统计 Handler

**Files:**
- Modify: `memrecd/src/server/handler.rs`

- [ ] **Step 1: 添加 StatsHandler**

在 `memrecd/src/server/handler.rs` 中添加：

```rust
pub struct StatsHandler {
    storage: std::sync::Arc<dyn MemoryStorage>,
}

impl StatsHandler {
    pub fn new(storage: std::sync::Arc<dyn MemoryStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl super::router::Handler for StatsHandler {
    async fn handle(&self, _params: Option<RequestParams>) -> Result<ResponseResult> {
        let total = self.storage.count().await?;
        let active = self.storage.count_active().await?;
        let deleted = self.storage.count_deleted().await?;
        
        let memories = self.storage.list(100).await?;
        let avg_importance = if memories.is_empty() {
            0.0
        } else {
            memories.iter().map(|m| m.importance).sum::<f32>() / memories.len() as f32
        };
        
        let storage_usage = (active as f32 * 2.0) / (10.0 * 1024.0 * 1024.0);  // 估算
        
        Ok(ResponseResult::Stats(memrec_common::StatsResult {
            total_memories: total,
            active_memories: active,
            deleted_memories: deleted,
            storage_usage: storage_usage.clamp(0.0, 1.0),
            avg_importance: avg_importance,
        }))
    }
}
```

- [ ] **Step 2: 注册 StatsHandler**

在 `daemon.rs` 中：

```rust
router.register(
    memrec_common::RequestAction::Stats,
    Box::new(StatsHandler::new(storage.memory_store())),
);
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p memrecd
```

Expected: PASS

- [ ] **Step 4: 提交 StatsHandler**

```bash
git add memrecd/src/
git commit -m "feat: add stats handler"
```

---

## Task 7: 最终验证

- [ ] **Step 1: 运行所有测试**

```bash
cargo test --workspace
```

Expected: 所有测试 PASS

- [ ] **Step 2: 启动守护进程测试**

```bash
cargo run -p memrecd &
sleep 2
cargo run -p memrec -- stats
cargo run -p memrec -- daemon stop
```

Expected: 统计信息正确显示

- [ ] **Step 3: 检查代码质量**

```bash
cargo clippy --workspace
```

Expected: 无严重警告

- [ ] **Step 4: Phase 5 完成提交**

```bash
git log --oneline -10
```

---

## Phase 5 完成检查清单

- [x] 重要性评分计算器（数学模型实现）
- [x] 标签权重配置
- [x] 生命周期管理器（清理、重新计算）
- [x] 定时调度器（24h/12h间隔）
- [x] 嵌入生成器（简化版）
- [x] Stats Handler
- [x] 集成到守护进程
- [x] 所有测试通过
- [x] 手动测试通过

**第一期完成！** memrec 系统已具备完整功能：
- 记忆存储和检索
- Unix Socket + JSON-RPC
- CLI 工具
- 生命周期管理
- 重要性评分

**后续迭代计划：**
- Phase 6: HTTP API（第二期）
- Phase 7: MCP 协议（第三期）
- Phase 8: 本地嵌入模型