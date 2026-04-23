use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MemoryType {
    #[default]
    Conversation,
    Knowledge,
    Decision,
    Preference,
    Context,
}


impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::Conversation => write!(f, "conversation"),
            MemoryType::Knowledge => write!(f, "knowledge"),
            MemoryType::Decision => write!(f, "decision"),
            MemoryType::Preference => write!(f, "preference"),
            MemoryType::Context => write!(f, "context"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub memory_type: MemoryType,
    pub content: String,
    pub summary: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub importance: f32,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub project_id: Option<Uuid>,
    pub is_deleted: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
}

impl Memory {
    pub fn new(content: String, memory_type: MemoryType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            memory_type,
            content,
            summary: None,
            embedding: None,
            importance: 0.8,
            created_at: now,
            last_accessed: now,
            access_count: 0,
            tags: Vec::new(),
            metadata: HashMap::new(),
            project_id: None,
            is_deleted: false,
            deleted_at: None,
            chunk_group_id: None,
            chunk_index: None,
            chunk_total: None,
        }
    }
    
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
    
    pub fn with_project(mut self, project_id: Uuid) -> Self {
        self.project_id = Some(project_id);
        self
    }
    
    pub fn with_chunk_info(mut self, group_id: Uuid, index: u32, total: u32) -> Self {
        self.chunk_group_id = Some(group_id);
        self.chunk_index = Some(index);
        self.chunk_total = Some(total);
        self
    }
    
    pub fn access(&mut self) {
        self.last_accessed = Utc::now();
        self.access_count += 1;
    }
    
    pub fn is_chunked(&self) -> bool {
        self.chunk_group_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_memory_type_serde() {
        let types = [
            MemoryType::Conversation,
            MemoryType::Knowledge,
            MemoryType::Decision,
            MemoryType::Preference,
            MemoryType::Context,
        ];
        
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let parsed: MemoryType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, parsed);
        }
    }
    
    #[test]
    fn test_memory_type_json_values() {
        assert_eq!(serde_json::to_string(&MemoryType::Conversation).unwrap(), "\"conversation\"");
        assert_eq!(serde_json::to_string(&MemoryType::Knowledge).unwrap(), "\"knowledge\"");
    }
    
    #[test]
    fn test_memory_creation() {
        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge);
        
        assert!(!memory.id.to_string().is_empty());
        assert_eq!(memory.memory_type, MemoryType::Knowledge);
        assert_eq!(memory.content, "test content");
        assert!(memory.embedding.is_none());
        assert_eq!(memory.importance, 0.8);
        assert_eq!(memory.access_count, 0);
        assert!(memory.tags.is_empty());
        assert!(!memory.is_deleted);
    }
    
    #[test]
    fn test_memory_with_tags() {
        let memory = Memory::new("test".to_string(), MemoryType::Decision)
            .with_tags(vec!["important".to_string(), "project-x".to_string()]);
        
        assert_eq!(memory.tags.len(), 2);
        assert!(memory.tags.contains(&"important".to_string()));
    }
    
    #[test]
    fn test_memory_access() {
        let mut memory = Memory::new("test".to_string(), MemoryType::Conversation);
        let initial_accessed = memory.last_accessed;
        
        memory.access();
        
        assert!(memory.last_accessed > initial_accessed);
        assert_eq!(memory.access_count, 1);
    }
    
    #[test]
    fn test_memory_serde() {
        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["tag1".to_string()]);
        
        let json = serde_json::to_string(&memory).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();
        
        assert_eq!(memory.id, parsed.id);
        assert_eq!(memory.content, parsed.content);
        assert_eq!(memory.tags, parsed.tags);
    }
    
    #[test]
    fn test_memory_chunk_fields() {
        let group_id = Uuid::new_v4();
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge)
            .with_chunk_info(group_id, 0, 3);
        
        assert_eq!(memory.chunk_group_id, Some(group_id));
        assert_eq!(memory.chunk_index, Some(0));
        assert_eq!(memory.chunk_total, Some(3));
        assert!(memory.is_chunked());
    }
    
    #[test]
    fn test_memory_chunk_serde() {
        let group_id = Uuid::new_v4();
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge)
            .with_chunk_info(group_id, 1, 5);
        
        let json = serde_json::to_string(&memory).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();
        
        assert_eq!(memory.chunk_group_id, parsed.chunk_group_id);
        assert_eq!(memory.chunk_index, parsed.chunk_index);
        assert_eq!(memory.chunk_total, parsed.chunk_total);
    }
}