//! # 场景实体定义
//!
//! [`Scene`] 为 L2 聚合层，用于组织同主题的记忆卡片。
//!
//! ## 设计理念
//!
//! 基于 TencentDB Agent Memory 的记忆金字塔启发，L2 场景层在 L1 记忆卡片之上
//! 提供主题维度的聚合视图。场景通过热度值排序，热度基于关联记忆的访问频率计算。
//!
//! ## 热度计算
//!
//! ```text
//! heat(scene) = Σ memory.access_count * memory.importance
//!               -----------------------------------------
//!                      memory_ids.len()
//! ```
//!
//! 热度每日衰减 5%（乘以 0.95 系数），避免热度无限累积。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 场景实体，聚合同主题的记忆卡片。
///
/// 每个场景代表一个独立的主题领域，如"Rust 开发规范"、"JWT 认证方案"等。
/// 场景通过热度值排序，热度基于关联记忆的访问频率计算。
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// 软删除标记
    #[serde(default)]
    pub is_deleted: bool,

    /// 删除时间
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Scene {
    /// 创建新场景，初始热度为 0.0。
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
            is_deleted: false,
            deleted_at: None,
        }
    }

    /// 设置描述，返回自身以支持链式调用。
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// 设置标签，返回自身以支持链式调用。
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// 设置项目归属，返回自身以支持链式调用。
    pub fn with_project(mut self, project_id: Uuid) -> Self {
        self.project_id = Some(project_id);
        self
    }

    /// 添加记忆到场景，自动去重并更新时间戳。
    pub fn add_memory(&mut self, memory_id: Uuid) {
        if !self.memory_ids.contains(&memory_id) {
            self.memory_ids.push(memory_id);
            self.updated_at = Utc::now();
        }
    }

    /// 从场景移除记忆，自动更新时间戳。
    pub fn remove_memory(&mut self, memory_id: &Uuid) {
        let before_len = self.memory_ids.len();
        self.memory_ids.retain(|id| id != memory_id);
        if self.memory_ids.len() != before_len {
            self.updated_at = Utc::now();
        }
    }

    /// 计算热度值，基于关联记忆的访问频率和重要性。
    ///
    /// # 参数
    ///
    /// - `memories`: 所有关联记忆的切片（需调用者提供，因为 Scene 只存储 ID）
    ///
    /// # 返回
    ///
    /// 热度值 = Σ(access_count * importance) / memory_count
    pub fn calculate_heat(&self, memories: &[crate::Memory]) -> f32 {
        if self.memory_ids.is_empty() {
            return 0.0;
        }

        let total: f32 = memories
            .iter()
            .filter(|m| self.memory_ids.contains(&m.id))
            .map(|m| m.access_count as f32 * m.importance)
            .sum();

        total / self.memory_ids.len() as f32
    }

    /// 应用热度衰减（每日调用）。
    ///
    /// 衰减系数为 0.95，即保留 95% 的热度。
    pub fn decay_heat(&mut self, factor: f32) {
        self.heat *= factor;
        self.updated_at = Utc::now();
    }

    /// 判断场景是否为空（无关联记忆）。
    pub fn is_empty(&self) -> bool {
        self.memory_ids.is_empty()
    }

    /// 获取关联记忆数量。
    pub fn memory_count(&self) -> usize {
        self.memory_ids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryType;

    #[test]
    fn test_scene_creation() {
        let scene = Scene::new("Rust 开发规范".to_string());

        assert!(!scene.id.to_string().is_empty());
        assert_eq!(scene.theme, "Rust 开发规范");
        assert!(scene.memory_ids.is_empty());
        assert_eq!(scene.heat, 0.0);
        assert!(scene.description.is_none());
        assert!(scene.tags.is_empty());
        assert!(scene.project_id.is_none());
        assert!(!scene.is_deleted);
    }

    #[test]
    fn test_scene_with_options() {
        let project_id = Uuid::new_v4();
        let scene = Scene::new("Test Scene".to_string())
            .with_description("Test description".to_string())
            .with_tags(vec!["tag1".to_string(), "tag2".to_string()])
            .with_project(project_id);

        assert_eq!(scene.description, Some("Test description".to_string()));
        assert_eq!(scene.tags.len(), 2);
        assert_eq!(scene.project_id, Some(project_id));
    }

    #[test]
    fn test_scene_add_memory() {
        let mut scene = Scene::new("Test".to_string());
        let mem_id = Uuid::new_v4();

        scene.add_memory(mem_id);
        assert_eq!(scene.memory_ids.len(), 1);
        assert!(scene.memory_ids.contains(&mem_id));

        // 测试去重
        scene.add_memory(mem_id);
        assert_eq!(scene.memory_ids.len(), 1);
    }

    #[test]
    fn test_scene_remove_memory() {
        let mut scene = Scene::new("Test".to_string());
        let mem_id = Uuid::new_v4();

        scene.add_memory(mem_id);
        assert_eq!(scene.memory_ids.len(), 1);

        scene.remove_memory(&mem_id);
        assert_eq!(scene.memory_ids.len(), 0);
    }

    #[test]
    fn test_scene_calculate_heat() {
        let mut scene = Scene::new("Test".to_string());

        let mem1 = Memory::new("content 1".to_string(), MemoryType::Knowledge);
        let mem2 = Memory::new("content 2".to_string(), MemoryType::Knowledge);

        scene.add_memory(mem1.id);
        scene.add_memory(mem2.id);

        let memories = vec![mem1, mem2];
        let heat = scene.calculate_heat(&memories);

        // 初始 access_count = 0, importance = 0.8
        // heat = (0 * 0.8 + 0 * 0.8) / 2 = 0.0
        assert_eq!(heat, 0.0);
    }

    #[test]
    fn test_scene_calculate_heat_with_access() {
        let mut scene = Scene::new("Test".to_string());

        let mut mem1 = Memory::new("content 1".to_string(), MemoryType::Knowledge);
        mem1.access_count = 10;
        mem1.importance = 1.0;

        let mut mem2 = Memory::new("content 2".to_string(), MemoryType::Knowledge);
        mem2.access_count = 5;
        mem2.importance = 0.8;

        scene.add_memory(mem1.id);
        scene.add_memory(mem2.id);

        let memories = vec![mem1, mem2];
        let heat = scene.calculate_heat(&memories);

        // heat = (10 * 1.0 + 5 * 0.8) / 2 = (10 + 4) / 2 = 7.0
        assert!((heat - 7.0).abs() < 0.001);
    }

    #[test]
    fn test_scene_decay_heat() {
        let mut scene = Scene::new("Test".to_string());
        scene.heat = 10.0;

        scene.decay_heat(0.95);
        assert!((scene.heat - 9.5).abs() < 0.001);

        scene.decay_heat(0.95);
        assert!((scene.heat - 9.025).abs() < 0.001);
    }

    #[test]
    fn test_scene_serde() {
        let scene = Scene::new("Test".to_string())
            .with_tags(vec!["rust".to_string()]);

        let json = serde_json::to_string(&scene).unwrap();
        let parsed: Scene = serde_json::from_str(&json).unwrap();

        assert_eq!(scene.id, parsed.id);
        assert_eq!(scene.theme, parsed.theme);
        assert_eq!(scene.tags, parsed.tags);
    }

    #[test]
    fn test_scene_backward_compatibility() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "theme": "Test Scene",
            "memory_ids": [],
            "heat": 5.0,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "tags": []
        }"#;

        let scene: Scene = serde_json::from_str(json).unwrap();
        assert_eq!(scene.theme, "Test Scene");
        assert_eq!(scene.heat, 5.0);
        assert!(!scene.is_deleted);
        assert!(scene.deleted_at.is_none());
    }
}
