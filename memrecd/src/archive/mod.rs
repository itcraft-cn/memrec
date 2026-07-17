//! # 原始输入存档模块
//!
//! 提供 SQLite 持久化存档，记录所有原始输入，永不删除。
//!
//! ## 架构
//!
//! ```text
//! 用户 add 命令
//!     │
//!     ├─→ RocksDB 写入 (主存储，可被 Dream 删除)
//!     │
//!     └─→ SQLite 存档 (原始输入，永不删除)
//!             │
//!             └─→ 按年切分: raw_2026.db, raw_2027.db
//! ```
//!
//! ## 特点
//!
//! - **双写机制**: 每次添加记忆同时写入 RocksDB 和 SQLite
//! - **永不删除**: SQLite 存档不受 Dream 影响
//! - **按年切分**: 每年一个 SQLite 文件，防止单一文件过大
//! - **失败容错**: SQLite 写入失败不影响主流程（仅 warn 日志）

mod sqlite;

use anyhow::Result;
use chrono::Datelike;
use memrec_common::Memory;
use std::sync::Arc;

pub use sqlite::SqliteArchive;

/// 原始存档存储 trait
#[async_trait::async_trait]
pub trait ArchiveStorage: Send + Sync {
    /// 存档原始记忆
    async fn archive(&self, memory: &Memory) -> Result<()>;

    /// 查询存档（按时间范围）
    async fn query_range(
        &self,
        year: i32,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ArchivedMemory>>;

    /// 统计存档数量
    async fn count(&self, year: i32) -> Result<usize>;
}

/// 存档记忆结构（从 SQLite 读取）
#[derive(Debug, Clone)]
pub struct ArchivedMemory {
    pub id: String,
    pub content: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub project_id: Option<String>,
    pub is_global: bool,
    pub created_at: String,
    pub source: String,
    pub metadata: serde_json::Value,
}

/// 存档管理器
///
/// 负责管理按年切分的 SQLite 文件，自动选择正确的年份数据库。
pub struct ArchiveManager {
    data_dir: std::path::PathBuf,
    enabled: bool,
}

impl ArchiveManager {
    pub fn new(data_dir: std::path::PathBuf, enabled: bool) -> Self {
        Self { data_dir, enabled }
    }

    /// 获取指定年份的 SQLite 文件路径
    fn get_archive_path(&self, year: i32) -> std::path::PathBuf {
        self.data_dir.join("archive").join(format!("raw_{}.db", year))
    }

    /// 获取当前年份的存档实例
    pub fn get_current_archive(&self) -> Option<Arc<SqliteArchive>> {
        if !self.enabled {
            return None;
        }

        let year = chrono::Utc::now().year();
        let path = self.get_archive_path(year);

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match SqliteArchive::open(&path) {
            Ok(archive) => Some(Arc::new(archive)),
            Err(e) => {
                tracing::warn!("Failed to open archive {}: {}", path.display(), e);
                None
            }
        }
    }

    /// 存档记忆
    pub async fn archive(&self, memory: &Memory) {
        if !self.enabled {
            return;
        }

        let year = memory.created_at.year();
        let path = self.get_archive_path(year);

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match SqliteArchive::open(&path) {
            Ok(archive) => {
                if let Err(e) = archive.archive_sync(memory) {
                    tracing::warn!("Failed to archive memory {}: {}", memory.id, e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to open archive for year {}: {}", year, e);
            }
        }
    }

    /// 查询指定年份的存档
    pub fn get_archive(&self, year: i32) -> Result<Option<Arc<SqliteArchive>>> {
        if !self.enabled {
            return Ok(None);
        }

        let path = self.get_archive_path(year);
        if !path.exists() {
            return Ok(None);
        }

        let archive = SqliteArchive::open(&path)?;
        Ok(Some(Arc::new(archive)))
    }
}
