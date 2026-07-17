//! # SQLite 存档实现
//!
//! 使用 SQLite 存储原始输入，按年切分文件。

use crate::archive::{ArchiveStorage, ArchivedMemory};
use anyhow::{Context, Result};
use memrec_common::Memory;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

/// SQLite 存档存储
pub struct SqliteArchive {
    conn: Mutex<Connection>,
}

impl SqliteArchive {
    /// 打开或创建 SQLite 存档文件
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create archive directory: {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open archive: {}", path.display()))?;

        let archive = Self {
            conn: Mutex::new(conn),
        };
        archive.init_schema()?;
        Ok(archive)
    }

    /// 初始化数据库 schema
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS raw_inputs (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                memory_type TEXT NOT NULL,
                tags TEXT,
                project_id TEXT,
                is_global INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                source TEXT DEFAULT 'user',
                metadata TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_created_at ON raw_inputs(created_at);
            CREATE INDEX IF NOT EXISTS idx_project_id ON raw_inputs(project_id);
            CREATE INDEX IF NOT EXISTS idx_memory_type ON raw_inputs(memory_type);
            "#,
        ).context("Failed to initialize archive schema")?;

        Ok(())
    }

    /// 存档记忆（同步方法）
    pub fn archive_sync(&self, memory: &Memory) -> Result<()> {
        let tags_json = serde_json::to_string(&memory.tags)?;
        let metadata_json = serde_json::to_string(&memory.metadata)?;
        let is_global = if memory.scope == memrec_common::MemoryScope::Global {
            1
        } else {
            0
        };
        let project_id = memory.project_id.map(|id| id.to_string());
        let source = format!("{:?}", memory.source);

        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT OR REPLACE INTO raw_inputs 
                (id, content, memory_type, tags, project_id, is_global, created_at, source, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                memory.id.to_string(),
                memory.content,
                format!("{:?}", memory.memory_type),
                tags_json,
                project_id,
                is_global,
                memory.created_at.to_rfc3339(),
                source,
                metadata_json,
            ],
        ).context("Failed to insert archived memory")?;

        Ok(())
    }

    /// 查询存档
    pub fn query(&self, limit: usize, offset: usize) -> Result<Vec<ArchivedMemory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, content, memory_type, tags, project_id, is_global, created_at, source, metadata
            FROM raw_inputs
            ORDER BY created_at DESC
            LIMIT ?1 OFFSET ?2
            "#,
        )?;

        let rows = stmt.query_map(params![limit as i32, offset as i32], |row| {
            let tags_json: String = row.get(3)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let metadata_json: String = row.get(8)?;
            let metadata: serde_json::Value = serde_json::from_str(&metadata_json).unwrap_or(serde_json::Value::Null);
            let is_global: i32 = row.get(5)?;

            Ok(ArchivedMemory {
                id: row.get(0)?,
                content: row.get(1)?,
                memory_type: row.get(2)?,
                tags,
                project_id: row.get(4)?,
                is_global: is_global == 1,
                created_at: row.get(6)?,
                source: row.get(7)?,
                metadata,
            })
        })?;

        let mut memories = Vec::new();
        for row in rows {
            memories.push(row?);
        }

        Ok(memories)
    }

    /// 统计存档数量
    pub fn count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM raw_inputs",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// 按项目 ID 查询
    pub fn query_by_project(&self, project_id: &str, limit: usize) -> Result<Vec<ArchivedMemory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, content, memory_type, tags, project_id, is_global, created_at, source, metadata
            FROM raw_inputs
            WHERE project_id = ?1
            ORDER BY created_at DESC
            LIMIT ?2
            "#,
        )?;

        let rows = stmt.query_map(params![project_id, limit as i32], |row| {
            let tags_json: String = row.get(3)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let metadata_json: String = row.get(8)?;
            let metadata: serde_json::Value = serde_json::from_str(&metadata_json).unwrap_or(serde_json::Value::Null);
            let is_global: i32 = row.get(5)?;

            Ok(ArchivedMemory {
                id: row.get(0)?,
                content: row.get(1)?,
                memory_type: row.get(2)?,
                tags,
                project_id: row.get(4)?,
                is_global: is_global == 1,
                created_at: row.get(6)?,
                source: row.get(7)?,
                metadata,
            })
        })?;

        let mut memories = Vec::new();
        for row in rows {
            memories.push(row?);
        }

        Ok(memories)
    }
}

#[async_trait::async_trait]
impl ArchiveStorage for SqliteArchive {
    async fn archive(&self, memory: &Memory) -> Result<()> {
        self.archive_sync(memory)
    }

    async fn query_range(
        &self,
        _year: i32,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ArchivedMemory>> {
        self.query(limit, offset)
    }

    async fn count(&self, _year: i32) -> Result<usize> {
        self.count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memrec_common::{MemoryScope, MemorySource, MemoryType};
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn test_archive_and_query() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let archive = SqliteArchive::open(&path).unwrap();

        let memory = Memory {
            id: Uuid::new_v4(),
            content: "Test content".to_string(),
            memory_type: MemoryType::Knowledge,
            tags: vec!["test".to_string()],
            project_id: Some(Uuid::new_v4()),
            scope: MemoryScope::Project,
            source: MemorySource::User,
            importance: 0.5,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            access_count: 1,
            summary: None,
            embedding: None,
            metadata: std::collections::HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            chunk_group_id: None,
            chunk_index: None,
            chunk_total: None,
        };

        archive.archive_sync(&memory).unwrap();

        let memories = archive.query(10, 0).unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "Test content");
    }

    #[test]
    fn test_count() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let archive = SqliteArchive::open(&path).unwrap();

        assert_eq!(archive.count().unwrap(), 0);

        let memory = Memory {
            id: Uuid::new_v4(),
            content: "Test".to_string(),
            memory_type: MemoryType::Knowledge,
            tags: vec![],
            project_id: None,
            scope: MemoryScope::Global,
            source: MemorySource::User,
            importance: 0.5,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            access_count: 1,
            summary: None,
            embedding: None,
            metadata: std::collections::HashMap::new(),
            is_deleted: false,
            deleted_at: None,
            chunk_group_id: None,
            chunk_index: None,
            chunk_total: None,
        };

        archive.archive_sync(&memory).unwrap();
        assert_eq!(archive.count().unwrap(), 1);
    }
}
