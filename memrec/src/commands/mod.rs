//! # CLI 命令实现
//!
//! 各子命令的具体逻辑，通过 [`Client`] 与守护进程通信。

mod eval;
mod memory;
pub mod scene;
mod search;

pub use eval::eval_retrieval;
pub use memory::{
    add, delete, dream, dream_report, get, list, restore, stats, version, AddOptions, ListOptions,
};
pub use search::{search_execute, SearchArgs};

use anyhow::Result;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// 记忆检索范围。
///
/// `list` / `search` 子命令的三态范围参数（`-p/--project`、`-g/--global`、`-a/--all`）
/// 解析后的结果。显式参数优先；均未指定时按项目标记文件 `.mr_pid` 决定默认范围：
/// 存在则视为已注册项目（`Project`），否则视为公共记忆场景（`Global`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// 仅当前项目记忆
    Project,
    /// 仅公共（全局）记忆
    Global,
    /// 跨所有项目
    All,
}

/// 项目标记文件名（用户语境中的 `.mr_id`）。
///
/// 文件位于项目根目录，由守护进程的 `detect_project_id` 维护，
/// 内容格式为 `memrec_project_id=<uuid>`，存在即表示当前目录已注册为 memrec 项目。
pub const PROJECT_MARKER: &str = ".mr_pid";

/// 检测当前工作目录，优先使用 Git 仓库根目录。
pub fn detect_working_dir() -> Result<String> {
    let current = std::env::current_dir()?;

    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&current)
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    Ok(current.to_string_lossy().to_string())
}

/// 解析检索范围：显式三态参数优先，否则按项目标记文件 `.mr_pid` 判断。
///
/// 三个参数由 clap `ArgGroup` 保证互斥（最多一个为 true）。
pub fn resolve_scope(project: bool, global: bool, all: bool) -> Scope {
    if project {
        Scope::Project
    } else if global {
        Scope::Global
    } else if all {
        Scope::All
    } else {
        default_scope()
    }
}

/// 未指定范围参数时的默认范围：按项目根目录 `.mr_pid` 是否存在推断。
///
/// 存在 → 已注册项目（`Project`）；不存在 → 公共记忆场景（`Global`）。
/// 供 CLI 与 MCP 工具共用，保证两入口行为一致。
pub fn default_scope() -> Scope {
    if project_marker_exists() {
        Scope::Project
    } else {
        Scope::Global
    }
}

/// 当前工作目录的项目根目录是否存在 `.mr_pid` 标记文件。
fn project_marker_exists() -> bool {
    detect_working_dir()
        .map(|dir| PathBuf::from(dir).join(PROJECT_MARKER).exists())
        .unwrap_or(false)
}

/// 从 `.mr_pid` 文件解析项目 UUID。
///
/// 文件格式为 `memrec_project_id=<uuid>`（由守护进程写入），
/// 解析失败（文件缺失/格式异常）时返回 `None`，调用方按未注册项目处理。
pub fn read_project_uuid(root: &Path) -> Option<Uuid> {
    let content = std::fs::read_to_string(root.join(PROJECT_MARKER)).ok()?;
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() == "memrec_project_id" {
            Uuid::parse_str(value.trim()).ok()
        } else {
            None
        }
    })
}
