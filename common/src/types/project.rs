use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::MemoryConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub config: ProjectConfig,
}

impl Project {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description: None,
            created_at: Utc::now(),
            config: ProjectConfig::default(),
        }
    }
    
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
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
        let project = Project::new("test".to_string())
            .with_description("Test project".to_string());
        
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