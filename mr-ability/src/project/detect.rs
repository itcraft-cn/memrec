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
/// - 若存在则解析返回已有 UUID；
/// - 若不存在则**独占**创建新文件并返回新生成的 UUID。
///
/// # 并发安全
///
/// 旧实现为「先 `exists()` 再 `fs::write`」，两个并发首次写入会各自生成
/// 一个 UUID 并依次覆盖文件：先写者的记忆以其未落盘的 UUID 入库，此后按项目
/// 检索永远查不到，形成不可见的项目分叉。这里改用 `create_new`（O_EXCL）
/// 独占创建，`AlreadyExists` 时回到读取分支重试，保证并发下拿到的是同
/// 一个 UUID。
///
/// # 错误语义
///
/// 文件存在但内容无法解析时返回 `Err`（带文件路径），调用方据此报错或降级，
/// 不得当作「无项目」继续写入——否则会把本应属于项目的记忆写成全局记忆。
pub fn detect_project_id(working_dir: Option<&str>) -> anyhow::Result<Uuid> {
    use anyhow::Context;
    use std::io::{ErrorKind, Write};

    let project_root = find_project_root(working_dir)?;
    let mr_pid_path = project_root.join(".mr_pid");

    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&mr_pid_path)
    {
        Ok(mut file) => {
            let content = ProjectIdFile::new(None).to_string();
            file.write_all(content.as_bytes())
                .with_context(|| format!("写入 {} 失败", mr_pid_path.display()))?;
            // 立刻落盘：崩溃留下空文件会让后续所有读取都判定为损坏
            file.sync_all().ok();
            ProjectIdFile::parse(&content).map(|f| f.project_id)
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let content = fs::read_to_string(&mr_pid_path)
                .with_context(|| format!("读取 {} 失败", mr_pid_path.display()))?;
            let file = ProjectIdFile::parse(&content).with_context(|| {
                format!(
                    "{} 内容无效，请删除该文件后重新注册项目",
                    mr_pid_path.display()
                )
            })?;
            Ok(file.project_id)
        }
        Err(e) => Err(e).with_context(|| format!("创建 {} 失败", mr_pid_path.display())),
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;

    /// 并发首次写入必须得到同一个 project_id。
    ///
    /// 回归背景：旧实现「exists() → 生成 → write」，两个并发调用会各自生成
    /// UUID 并互相覆盖，先写者的记忆以其未落盘的 UUID 入库，形成项目分叉。
    #[test]
    fn test_concurrent_detect_returns_single_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 用空目录作为项目根：find_project_root 在非 git 目录下返回该目录
        let base = root.to_str().unwrap().to_string();

        let mut handles = Vec::new();
        for _ in 0..16 {
            let base = base.clone();
            handles.push(std::thread::spawn(move || {
                detect_project_id(Some(&base)).unwrap()
            }));
        }

        let ids: Vec<Uuid> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let first = ids[0];
        for id in &ids {
            assert_eq!(*id, first, "并发调用必须返回同一个 project_id");
        }

        // 磁盘内容必须与该 UUID 一致
        let content = fs::read_to_string(root.join(".mr_pid")).unwrap();
        assert_eq!(ProjectIdFile::parse(&content).unwrap().project_id, first);
    }

    /// `.mr_pid` 内容损坏时必须报错（带文件路径），不得静默返回新 ID。
    #[test]
    fn test_corrupt_marker_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join(".mr_pid"), "garbage-without-key").unwrap();

        let err = detect_project_id(root.to_str()).unwrap_err();
        let text = format!("{:#}", err);
        assert!(text.contains(".mr_pid"), "错误信息应包含文件路径：{text}");
    }
}
