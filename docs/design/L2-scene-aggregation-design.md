# L2 场景聚合层设计文档

> 状态：草稿  
> 日期：2026-08-04  
> 基于：TencentDB Agent Memory 记忆金字塔启发

## 1. 背景与目标

### 1.1 问题陈述

当前 memrec 的记忆为扁平结构（L1 层），仅有类型分类（decision/knowledge/context/preference/conversation），缺少场景维度的聚合。用户在检索时面临：

1. **信息碎片化**：相关记忆分散在不同时间点，需多次检索拼凑
2. **热度不可见**：无法识别高频访问的场景，优先级不明确
3. **情境缺失**：记忆按时间排序，无法按主题组织

### 1.2 设计目标

引入 **L2 场景聚合层**，实现：

1. **场景聚合**：按主题/标签聚合 L1 记忆卡片
2. **热度排序**：基于访问频率动态排序场景
3. **检索增强**：先定位场景再展开查 L1，提升检索精度
4. **证据链追踪**：从场景可追溯到原始记忆卡片

### 1.3 非目标

- **不引入 LLM**：场景聚合基于标签/语义，无需 LLM 提炼
- **不替代 L1**：场景为聚合视图，L1 记忆仍为核心存储
- **不实现 L3**：人格摘要层暂不实现（需理解语义）

## 2. 核心概念

### 2.1 记忆金字塔分层

```
L0 流水账层   ← 原始对话（memrec-mcp MCP 调用记录）
L1 记忆卡片层 ← 结构化事实（当前 Memory 实体） ✓ 已实现
L2 场景档案层 ← 主题聚合（新增 Scene 实体）  ★ 本设计
L3 人格摘要层 ← 稳定画像（跨场景画像）       ✗ 暂不实现
```

### 2.2 Scene 实体定义

```rust
/// 场景实体，聚合同主题的记忆卡片。
///
/// 每个场景代表一个独立的主题领域，如"Rust 开发规范"、"JWT 认证方案"等。
/// 场景通过热度值排序，热度基于关联记忆的访问频率计算。
pub struct Scene {
    /// 场景唯一标识
    pub id: Uuid,
    
    /// 场景主题（如"Rust 开发规范"）
    pub theme: String,
    
    /// 关联的记忆卡片 ID 列表
    pub memory_ids: Vec<Uuid>,
    
    /// 热度值（访问频率加权）
    pub heat: f32,
    
    /// 创建时间
    pub created_at: DateTime<Utc>,
    
    /// 最后更新时间
    pub updated_at: DateTime<Utc>,
    
    /// 场景描述（可选，用户自定义）
    pub description: Option<String>,
    
    /// 场景标签（用于聚类和检索）
    pub tags: Vec<String>,
    
    /// 项目归属（None 为全局场景）
    pub project_id: Option<Uuid>,
}
```

### 2.3 热度计算机制

热度值基于关联记忆的访问频率计算：

```
heat(scene) = Σ memory.access_count * memory.importance
              -----------------------------------------
                     memory_ids.len()
```

热度更新时机：
1. **实时更新**：记忆被访问（search/get）时，更新所属场景热度
2. **定期衰减**：每日凌晨执行热度衰减（乘以 0.95 系数）

## 3. 存储设计

### 3.1 RocksDB 列族设计

| 列族名       | 键格式              | 值格式        | 说明               |
|--------------|---------------------|---------------|--------------------|
| `scenes`     | `<scene_id>`        | JSON Scene    | 场景主表           |
| `by_scene`   | `<scene_id>:<mem_id>` | `<mem_id>`  | 场景-记忆索引      |
| `by_heat`    | `<heat>:<scene_id>` | `<scene_id>`  | 热度索引（降序）   |
| `by_theme`   | `<theme>:<scene_id>` | `<scene_id>`  | 主题索引           |

### 3.2 索引设计

#### 主索引：场景 ID

```
scenes 列族
键：scene_id.to_string().into_bytes()
值：serde_json::to_vec(&scene)
```

#### 二级索引：场景-记忆关联

```
by_scene 列族
键：format!("{}:{}", scene_id, memory_id)
值：memory_id.to_string().into_bytes()
```

用途：快速查询记忆所属场景

#### 三级索引：热度排序

```
by_heat 列族
键：format!("{:010.4}:{}", heat, scene_id)  // 热度格式化为 10 位小数，支持降序遍历
值：scene_id.to_string().into_bytes()
```

用途：按热度列出热门场景

#### 四级索引：主题检索

```
by_theme 列族
键：format!("{}:{}", theme, scene_id)
值：scene_id.to_string().into_bytes()
```

用途：按主题名称精确检索场景

## 4. 接口设计

### 4.1 CLI 命令

#### 创建场景

```bash
memrec scene create <theme> [--description <desc>] [--tag <tag>...] [--project <id>]
```

示例：
```bash
memrec scene create "Rust 开发规范" \
  --description "项目的 Rust 编码规范集合" \
  --tag rust --tag best-practice
```

输出：
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "theme": "Rust 开发规范",
  "memory_ids": [],
  "heat": 0.0
}
```

#### 添加记忆到场景

```bash
memrec scene add <scene_id> <memory_id> [<memory_id>...]
```

示例：
```bash
memrec scene add 550e8400... 123e4567-e89b-12d3-a456-426614174000
```

#### 从场景移除记忆

```bash
memrec scene remove <scene_id> <memory_id>
```

#### 列出场景

```bash
memrec scene list [--limit <n>] [--by-heat] [--project <id>]
```

示例：
```bash
memrec scene list --by-heat --limit 10
```

输出：
```json
[
  {
    "id": "550e8400-...",
    "theme": "Rust 开发规范",
    "memory_count": 5,
    "heat": 12.5
  }
]
```

#### 查看场景详情

```bash
memrec scene get <scene_id> [--expand]
```

示例：
```bash
memrec scene get 550e8400... --expand
```

输出（带展开的记忆）：
```json
{
  "id": "550e8400-...",
  "theme": "Rust 开发规范",
  "heat": 12.5,
  "memories": [
    {
      "id": "123e4567-...",
      "content": "使用 RAII 管理资源",
      "memory_type": "knowledge",
      "importance": 0.9
    }
  ]
}
```

#### 在场景内检索

```bash
memrec search <query> --scene <scene_id>
```

示例：
```bash
memrec search "资源管理" --scene 550e8400...
```

行为：仅检索场景关联的记忆卡片

#### 删除场景

```bash
memrec scene delete <scene_id>
```

注意：仅删除场景实体，不删除关联的 L1 记忆

### 4.2 RPC 接口

在 `mr-protocol` 中新增场景相关 RPC 方法：

```rust
pub enum Request {
    // ... 现有方法
    
    // 场景管理
    SceneCreate {
        theme: String,
        description: Option<String>,
        tags: Vec<String>,
        project_id: Option<Uuid>,
    },
    SceneAddMemories {
        scene_id: Uuid,
        memory_ids: Vec<Uuid>,
    },
    SceneRemoveMemory {
        scene_id: Uuid,
        memory_id: Uuid,
    },
    SceneList {
        limit: usize,
        sort_by_heat: bool,
        project_id: Option<Uuid>,
    },
    SceneGet {
        scene_id: Uuid,
        expand_memories: bool,
    },
    SceneDelete {
        scene_id: Uuid,
    },
    
    // 场景内检索
    SearchInScene {
        query: String,
        scene_id: Uuid,
        limit: usize,
    },
}
```

### 4.3 存储层接口

在 `mr-ability/src/storage/traits.rs` 新增 trait：

```rust
/// 场景存储接口
#[async_trait]
pub trait SceneStorage {
    /// 保存场景
    async fn save_scene(&self, scene: &Scene) -> Result<()>;
    
    /// 获取场景
    async fn get_scene(&self, id: &Uuid) -> Result<Option<Scene>>;
    
    /// 删除场景
    async fn delete_scene(&self, id: &Uuid) -> Result<bool>;
    
    /// 列出场景（按热度排序）
    async fn list_scenes(&self, limit: usize, sort_by_heat: bool) -> Result<Vec<Scene>>;
    
    /// 添加记忆到场景
    async fn add_memory_to_scene(&self, scene_id: &Uuid, memory_id: &Uuid) -> Result<()>;
    
    /// 从场景移除记忆
    async fn remove_memory_from_scene(&self, scene_id: &Uuid, memory_id: &Uuid) -> Result<()>;
    
    /// 更新场景热度
    async fn update_scene_heat(&self, scene_id: &Uuid) -> Result<()>;
    
    /// 查询记忆所属场景
    async fn get_memory_scenes(&self, memory_id: &Uuid) -> Result<Vec<Uuid>>;
}
```

## 5. 实现路径

### 5.1 数据模型实现

**文件**：`mr-common/src/types/scene.rs`

```rust
//! # 场景实体定义
//!
//! 场景为 L2 聚合层，用于组织同主题的记忆卡片。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 场景实体，聚合同主题的记忆卡片。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: Uuid,
    pub theme: String,
    pub memory_ids: Vec<Uuid>,
    pub heat: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub project_id: Option<Uuid>,
}

impl Scene {
    /// 创建新场景
    pub fn new(theme: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            theme,
            memory_ids: Vec::new(),
            heat: 0.0,
            created_at: now,
            updated_at: now,
            description: None,
            tags: Vec::new(),
            project_id: None,
        }
    }
    
    /// 设置描述
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
    
    /// 设置标签
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
    
    /// 设置项目归属
    pub fn with_project(mut self, project_id: Uuid) -> Self {
        self.project_id = Some(project_id);
        self
    }
    
    /// 添加记忆到场景
    pub fn add_memory(&mut self, memory_id: Uuid) {
        if !self.memory_ids.contains(&memory_id) {
            self.memory_ids.push(memory_id);
            self.updated_at = Utc::now();
        }
    }
    
    /// 从场景移除记忆
    pub fn remove_memory(&mut self, memory_id: &Uuid) {
        self.memory_ids.retain(|id| id != memory_id);
        self.updated_at = Utc::now();
    }
    
    /// 计算热度
    pub fn calculate_heat(&self, memories: &[Memory]) -> f32 {
        if memories.is_empty() {
            return 0.0;
        }
        
        let total: f32 = memories
            .iter()
            .filter(|m| self.memory_ids.contains(&m.id))
            .map(|m| m.access_count as f32 * m.importance)
            .sum();
        
        total / self.memory_ids.len() as f32
    }
}
```

### 5.2 存储层实现

**文件**：`mr-ability/src/storage/scene_store.rs`

```rust
//! # 场景存储实现
//!
//! [`SceneStore`] 基于 [`RocksDBStore`] 实现 [`SceneStorage`] trait。

use super::rocksdb::RocksDBStore;
use super::traits::SceneStorage;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mr_common::Scene;
use uuid::Uuid;

pub struct SceneStore {
    rocksdb: RocksDBStore,
}

impl SceneStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }
    
    fn scene_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }
    
    fn serialize_scene(scene: &Scene) -> Result<Vec<u8>> {
        serde_json::to_vec(scene).context("Failed to serialize scene")
    }
    
    fn deserialize_scene(data: &[u8]) -> Result<Scene> {
        serde_json::from_slice(data).context("Failed to deserialize scene")
    }
}

#[async_trait]
impl SceneStorage for SceneStore {
    async fn save_scene(&self, scene: &Scene) -> Result<()> {
        let id_key = Self::scene_key(&scene.id);
        let data = Self::serialize_scene(scene)?;
        
        let cf_scenes = self.rocksdb.cf_scenes()?;
        self.rocksdb.put_cf(cf_scenes, &id_key, &data)?;
        
        // 更新热度索引
        let heat_key = format!("{:010.4}:{}", scene.heat, scene.id);
        let cf_by_heat = self.rocksdb.cf_by_heat()?;
        self.rocksdb.put_cf(cf_by_heat, heat_key.as_bytes(), &id_key)?;
        
        Ok(())
    }
    
    async fn get_scene(&self, id: &Uuid) -> Result<Option<Scene>> {
        let id_key = Self::scene_key(id);
        let cf_scenes = self.rocksdb.cf_scenes()?;
        
        match self.rocksdb.get_cf(cf_scenes, &id_key)? {
            Some(bytes) => {
                let scene = Self::deserialize_scene(&bytes)?;
                Ok(Some(scene))
            }
            None => Ok(None),
        }
    }
    
    // ... 其他方法实现
}
```

### 5.3 CLI 集成

**文件**：`memrec/src/commands/scene.rs`

```rust
//! # 场景管理命令

use clap::Subcommand;
use mr_common::Scene;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum SceneCommand {
    /// 创建场景
    Create {
        /// 场景主题
        theme: String,
        
        /// 场景描述
        #[arg(short, long)]
        description: Option<String>,
        
        /// 场景标签
        #[arg(short, long)]
        tag: Vec<String>,
    },
    
    /// 添加记忆到场景
    Add {
        /// 场景 ID
        scene_id: String,
        
        /// 记忆 ID 列表
        memory_ids: Vec<String>,
    },
    
    /// 列出场景
    List {
        /// 限制数量
        #[arg(short, long, default_value = "20")]
        limit: usize,
        
        /// 按热度排序
        #[arg(long)]
        by_heat: bool,
    },
    
    /// 查看场景详情
    Get {
        /// 场景 ID
        scene_id: String,
        
        /// 展开记忆详情
        #[arg(short, long)]
        expand: bool,
    },
    
    /// 删除场景
    Delete {
        /// 场景 ID
        scene_id: String,
    },
}

pub async fn handle_scene_command(cmd: SceneCommand) -> Result<()> {
    match cmd {
        SceneCommand::Create { theme, description, tag } => {
            // RPC 调用 SceneCreate
        }
        SceneCommand::Add { scene_id, memory_ids } => {
            // RPC 调用 SceneAddMemories
        }
        // ... 其他命令
    }
    Ok(())
}
```

### 5.4 热度衰减机制

**文件**：`memrecd/src/scheduler/heat_decay.rs`

```rust
//! # 热度衰减调度器
//!
//! 定期执行场景热度衰减，避免热度无限累积。

use std::time::Duration;
use tokio::time::interval;

/// 热度衰减系数（每日衰减 5%）
const HEAT_DECAY_FACTOR: f32 = 0.95;

/// 启动热度衰减调度器
pub async fn start_heat_decay_scheduler(scene_store: Arc<SceneStore>) {
    let mut interval = interval(Duration::from_secs(86400)); // 每日执行
    
    loop {
        interval.tick().await;
        
        if let Err(e) = decay_scene_heat(&scene_store).await {
            log::error!("Failed to decay scene heat: {}", e);
        }
    }
}

async fn decay_scene_heat(scene_store: &SceneStore) -> Result<()> {
    let scenes = scene_store.list_scenes(10000, false).await?;
    
    for mut scene in scenes {
        scene.heat *= HEAT_DECAY_FACTOR;
        scene_store.save_scene(&scene).await?;
    }
    
    log::info!("Decayed heat for {} scenes", scenes.len());
    Ok(())
}
```

## 6. 测试计划

### 6.1 单元测试

- `Scene::new()` 正确初始化字段
- `Scene::add_memory()` 去重添加
- `Scene::calculate_heat()` 正确计算热度
- `SceneStore::save/get` 正确序列化/反序列化

### 6.2 集成测试

- 创建场景并添加记忆
- 按热度列出场景
- 在场景内检索记忆
- 热度更新与衰减

### 6.3 性能测试

- 场景创建延迟（< 5ms）
- 热度排序查询延迟（< 10ms）
- 1000 个场景的热度衰减耗时（< 100ms）

## 7. 迁移与兼容性

### 7.1 数据迁移

无需迁移历史数据，L2 为新增层，与现有 L1 记忆独立。

### 7.2 兼容性保证

- **向后兼容**：现有 API 不受影响
- **渐进式采用**：用户可选择性使用场景功能

## 8. 后续扩展

### 8.1 自动场景聚类

基于记忆标签的相似度，自动聚类生成场景（需语义支持）。

### 8.2 场景推荐

根据用户查询意图，推荐相关场景（基于语义匹配）。

### 8.3 场景导出

将场景及其关联记忆导出为 Markdown 文档，便于阅读和分享。

## 9. 参考资料

- TencentDB Agent Memory 设计文档
- MemRec 架构设计文档（mr-common/src/types/mod.rs）
- RocksDB 列族设计最佳实践