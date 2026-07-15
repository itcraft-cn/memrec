//! # 项目隔离模型
//!
//! 定义项目实体 [`Project`] 和项目级配置 [`ProjectConfig`]，
//! 实现多项目独立记忆空间。
//!
//! ## 项目隔离机制
//!
//! 每个项目拥有唯一的 [`Project::id`]，记忆通过 `project_id` 字段关联到项目。
//! 公共记忆（`project_id` 为 `None`）对所有项目可见。
//! 同一时刻仅一个项目为活跃状态（`ProjectConfig::active == true`）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::MemoryConfig;

/// 项目实体，代表一个独立的项目记忆空间。
///
/// 每个项目拥有独立的记忆管理配置（[`ProjectConfig`]），
/// 可覆盖全局默认配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub config: ProjectConfig,
}

impl Project {
    /// 创建新项目，自动生成 UUID，配置使用默认值。
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description: None,
            created_at: Utc::now(),
            config: ProjectConfig::default(),
        }
    }

    /// 设置项目描述，返回自身以支持链式调用。
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
}

/// 项目级配置。
///
/// 每个项目可拥有独立的记忆管理策略（覆盖全局默认值），
/// `active` 标记当前活跃项目（同一时刻仅一个为 true）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    pub memory_config: MemoryConfig,
    pub active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_creation() {
        let project = Project::new("my-project".to_string());

        assert!(!project.id.to_string().is_empty());
        assert_eq!(project.name, "my-project");
        assert!(project.description.is_none());
        assert!(!project.config.active);
    }

    #[test]
    fn test_project_with_description() {
        let project = Project::new("test".to_string()).with_description("Test project".to_string());

        assert_eq!(project.description, Some("Test project".to_string()));
    }

    #[test]
    fn test_project_serde() {
        let project = Project::new("test".to_string());
        let json = serde_json::to_string(&project).unwrap();
        let parsed: Project = serde_json::from_str(&json).unwrap();

        assert_eq!(project.id, parsed.id);
        assert_eq!(project.name, parsed.name);
    }
}
