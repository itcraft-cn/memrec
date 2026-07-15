//! # 项目 ID 检测与 `.mr_pid` 文件管理
//!
//! 每个项目通过 `.mr_pid` 文件持有唯一 UUID，实现记忆的项目级隔离。
//!
//! ## `.mr_pid` 文件格式
//!
//! ```ini
//! memrec_project_id=550e8400-e29b-41d4-a716-446655440000
//! created_at=2024-01-01T00:00:00Z
//! project_name=my-project   # 可选
//! ```
//!
//! ## 项目根目录检测
//!
//! 优先使用 `git rev-parse --show-toplevel` 确定项目根目录，
//! 若不在 Git 仓库中则使用当前工作目录。

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

/// `.mr_pid` 文件内容模型。
///
/// 存储项目的唯一标识、创建时间和可选的项目名称。
pub struct ProjectIdFile {
    pub project_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub project_name: Option<String>,
}

impl ProjectIdFile {
    /// 创建新的项目 ID 文件，生成随机 UUID。
    pub fn new(project_name: Option<String>) -> Self {
        Self {
            project_id: Uuid::new_v4(),
            created_at: Utc::now(),
            project_name,
        }
    }

    /// 从 `.mr_pid` 文件内容解析项目信息。
    ///
    /// 要求 `memrec_project_id` 和 `created_at` 字段必须存在，
    /// `project_name` 为可选项。
    pub fn parse(content: &str) -> Result<Self> {
        let mut project_id: Option<Uuid> = None;
        let mut created_at: Option<DateTime<Utc>> = None;
        let mut project_name: Option<String> = None;

        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "memrec_project_id" => {
                        project_id = Some(Uuid::parse_str(value.trim())?);
                    }
                    "created_at" => {
                        created_at =
                            Some(DateTime::parse_from_rfc3339(value.trim())?.with_timezone(&Utc));
                    }
                    "project_name" => {
                        project_name = Some(value.trim().to_string());
                    }
                    _ => {}
                }
            }
        }

        match (project_id, created_at) {
            (Some(id), Some(at)) => Ok(Self {
                project_id: id,
                created_at: at,
                project_name,
            }),
            _ => anyhow::bail!("Invalid .mr_pid file format"),
        }
    }
}

impl fmt::Display for ProjectIdFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "memrec_project_id={}\ncreated_at={}",
            self.project_id,
            self.created_at.to_rfc3339()
        )?;
        if let Some(name) = &self.project_name {
            write!(f, "\nproject_name={}", name)?;
        }
        Ok(())
    }
}

/// 查找项目根目录。
///
/// 优先使用 `git rev-parse --show-toplevel`，失败时回退到当前工作目录。
pub fn find_project_root(working_dir: Option<&str>) -> Result<PathBuf> {
    let start_dir = if let Some(dir) = working_dir {
        PathBuf::from(dir)
    } else {
        std::env::current_dir()?
    };

    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&start_dir)
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    Ok(start_dir)
}

/// 检测或创建项目 ID。
///
/// 在项目根目录查找 `.mr_pid` 文件：
/// - 若存在则解析返回已有 UUID
/// - 若不存在则创建新文件并返回新生成的 UUID
pub fn detect_project_id(working_dir: Option<&str>) -> Result<Uuid> {
    let project_root = find_project_root(working_dir)?;
    let mr_pid_path = project_root.join(".mr_pid");

    if mr_pid_path.exists() {
        let content = fs::read_to_string(&mr_pid_path)?;
        let file = ProjectIdFile::parse(&content)?;
        Ok(file.project_id)
    } else {
        let file = ProjectIdFile::new(None);
        fs::write(&mr_pid_path, file.to_string())?;
        Ok(file.project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_project_id_file_creation() {
        let file = ProjectIdFile::new(Some("test-project".to_string()));
        assert!(!file.project_id.is_nil());
        assert_eq!(file.project_name, Some("test-project".to_string()));
    }

    #[test]
    fn test_project_id_file_parse() {
        let original = ProjectIdFile::new(None);
        let content = original.to_string();
        let parsed = ProjectIdFile::parse(&content).unwrap();

        assert_eq!(original.project_id, parsed.project_id);
    }

    #[test]
    fn test_project_id_file_roundtrip() {
        let original = ProjectIdFile::new(Some("my-project".to_string()));
        let content = original.to_string();
        let parsed = ProjectIdFile::parse(&content).unwrap();

        assert_eq!(original.project_id, parsed.project_id);
        assert_eq!(original.project_name, parsed.project_name);
    }
}
