//! # 记忆实体与记忆类型
//!
//! 定义 memrec 系统的核心数据单元 [`Memory`] 及其类型枚举 [`MemoryType`]。
//!
//! ## 记忆类型
//!
//! [`MemoryType`] 将记忆分为五类：对话、知识、决策、偏好、上下文，
//! 默认为 `Conversation`。序列化为小写字符串（如 `"knowledge"`）。
//!
//! ## 记忆实体
//!
//! [`Memory`] 是系统的核心数据单元，包含：
//!
//! - 内容与摘要（`content` / `summary`）
//! - 嵌入向量（`embedding`），由 ONNX Runtime 推理生成
//! - 重要性评分（`importance`），由 [`ImportanceConfig`](super::ImportanceConfig) 加权计算
//! - 分块信息（`chunk_group_id` / `chunk_index` / `chunk_total`），长文本分块存储
//! - 软删除标记（`is_deleted` / `deleted_at`），支持恢复期内的记忆恢复
//!
//! ## 构建器模式
//!
//! [`Memory`] 提供了 `with_*` 系列方法实现链式构建，便于在添加记忆时
//! 一次性设置标签、项目归属、分块信息等。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 记忆类型枚举。
///
/// 将记忆按语义分类，影响搜索权重和展示方式。
/// 序列化为小写字符串，默认为 `Conversation`。
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

/// 记忆实体，系统的核心数据单元。
///
/// 每条记忆拥有唯一 ID，可归属于某个项目（`project_id`），
/// 也可为公共记忆（`project_id` 为 `None`，对所有项目可见）。
/// 长文本可通过分块机制拆分为多条记忆，共享同一 `chunk_group_id`。
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

    #[serde(default)]
    pub source: MemorySource,

    #[serde(default)]
    pub scope: MemoryScope,
}

impl Memory {
    /// 创建新记忆，初始重要性为 0.8，访问计数为 0。
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
            source: MemorySource::default(),
            scope: MemoryScope::default(),
        }
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

    /// 设置分块信息，返回自身以支持链式调用。
    ///
    /// - `group_id`：同一组分块共享的组 ID
    /// - `index`：当前分块在组内的序号（从 0 开始）
    /// - `total`：组内分块总数
    pub fn with_chunk_info(mut self, group_id: Uuid, index: u32, total: u32) -> Self {
        self.chunk_group_id = Some(group_id);
        self.chunk_index = Some(index);
        self.chunk_total = Some(total);
        self
    }

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

    /// 记录一次访问，更新最后访问时间并递增访问计数。
    pub fn access(&mut self) {
        self.last_accessed = Utc::now();
        self.access_count += 1;
    }

    /// 判断该记忆是否属于某个分块组。
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
        assert_eq!(
            serde_json::to_string(&MemoryType::Conversation).unwrap(),
            "\"conversation\""
        );
        assert_eq!(
            serde_json::to_string(&MemoryType::Knowledge).unwrap(),
            "\"knowledge\""
        );
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
        let memory =
            Memory::new("test".to_string(), MemoryType::Knowledge).with_chunk_info(group_id, 0, 3);

        assert_eq!(memory.chunk_group_id, Some(group_id));
        assert_eq!(memory.chunk_index, Some(0));
        assert_eq!(memory.chunk_total, Some(3));
        assert!(memory.is_chunked());
    }

    #[test]
    fn test_memory_chunk_serde() {
        let group_id = Uuid::new_v4();
        let memory =
            Memory::new("test".to_string(), MemoryType::Knowledge).with_chunk_info(group_id, 1, 5);

        let json = serde_json::to_string(&memory).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();

        assert_eq!(memory.chunk_group_id, parsed.chunk_group_id);
        assert_eq!(memory.chunk_index, parsed.chunk_index);
        assert_eq!(memory.chunk_total, parsed.chunk_total);
    }

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
}
