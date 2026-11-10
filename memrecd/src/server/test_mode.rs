//! # 测试期禁写
//!
//! 端到端测试需要独占写入，但同一台机器上的其他 Agent 仍会通过 memrecd 生产
//! socket 读写记忆。本模块提供“测试模式”：
//!
//! - 开关文件 `~/.memrec/.debug` 存在即视为测试模式（带最大存活时间，防止残留文件
//!   永久阻断生产写入）；
//! - 守护进程同时监听生产 socket 与测试 socket；
//! - 测试模式生效时，**生产 socket 上的变更类请求**返回专用错误码 `-32010`，
//!   测试 socket 不受影响；
//! - 读请求不受限制，避免打断其他 Agent 的会话（如需只读隔离，另议）。
//!
//! 身份来自“连接的是哪个 socket”，而不是文件或请求字段：其他 Agent 的客户端
//! 继续使用生产 socket，天然无法绕过；测试端只需把 `MEMREC_SOCKET_PATH` 指向
//! 测试 socket。
//!
//! ## 为什么不用文件直接拒写
//!
//! 文件只能表达“处于测试态”，不能区分“谁可以写”。用 socket 做身份，服务端即可
//! 做强制校验，不依赖客户端自觉，也没有“检查后、发送前”的竞态窗口。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use mr_protocol::RequestAction;

/// 连接来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnMode {
    /// 生产 socket：测试模式下拒绝写入
    Production,
    /// 测试 socket：始终允许写入
    TestSocket,
}

/// 测试模式判定器。
#[derive(Debug, Clone)]
pub struct TestMode {
    /// 开关文件；`None` 表示禁用判定（单元测试默认）
    file: Option<PathBuf>,
    /// 开关文件最大存活时间，超时视为残留并忽略
    max_age: Duration,
}

impl TestMode {
    /// 禁用测试模式判定。
    pub fn disabled() -> Self {
        Self {
            file: None,
            max_age: Duration::ZERO,
        }
    }

    /// 使用指定开关文件与最大存活时间创建判定器。
    ///
    /// `max_age_secs = 0` 表示不设上限（不建议，崩溃后会永久残留）。
    pub fn new(file: PathBuf, max_age_secs: u64) -> Self {
        Self {
            file: Some(file),
            max_age: Duration::from_secs(max_age_secs),
        }
    }

    /// 开关文件路径（用于日志与自检）。
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// 当前是否处于测试模式。
    ///
    /// 文件不存在、不是普通文件、修改时间无法读取或已超过 `max_age` 时返回 false。
    pub fn active(&self) -> bool {
        let Some(path) = &self.file else {
            return false;
        };
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        if !meta.is_file() {
            return false;
        }
        if self.max_age.is_zero() {
            return true;
        }
        match meta.modified() {
            Ok(modified) => match SystemTime::now().duration_since(modified) {
                Ok(age) => age <= self.max_age,
                // 修改时间在未来（时钟回拨）：保守视为仍然有效
                Err(_) => true,
            },
            // 无法读取修改时间：保守视为有效
            Err(_) => true,
        }
    }
}

/// 是否为变更类请求（测试模式下生产 socket 拒绝）。
///
/// 采用穷举匹配：新增 [`RequestAction`] 变体时编译器会强制在此归类，
/// 避免新写接口漏过测试期禁写。
pub fn is_write_action(action: &RequestAction) -> bool {
    match action {
        // 读请求
        RequestAction::Get
        | RequestAction::Search
        | RequestAction::List
        | RequestAction::SearchMemory
        | RequestAction::TieredSearch
        | RequestAction::GetProjectInfo
        | RequestAction::GetVersion
        | RequestAction::DreamReport
        | RequestAction::ProjectList
        | RequestAction::ConfigGet
        | RequestAction::Stats
        | RequestAction::SceneGet
        | RequestAction::SceneList => false,

        // 变更请求
        RequestAction::Add
        | RequestAction::Update
        | RequestAction::Delete
        | RequestAction::Restore
        | RequestAction::Tag
        | RequestAction::Dream
        | RequestAction::ProjectCreate
        | RequestAction::ProjectSwitch
        | RequestAction::ProjectDelete
        | RequestAction::ConfigSet
        | RequestAction::SceneCreate
        | RequestAction::SceneDelete
        | RequestAction::SceneAddMemory
        | RequestAction::SceneRemoveMemory
        | RequestAction::SceneUpdateHeat => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_never_active() {
        assert!(!TestMode::disabled().active());
    }

    #[test]
    fn test_active_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".debug");
        let mode = TestMode::new(file.clone(), 3600);
        assert!(!mode.active());
        std::fs::write(&file, b"{}").unwrap();
        assert!(mode.active());
        std::fs::remove_file(&file).unwrap();
        assert!(!mode.active());
    }

    #[test]
    fn test_stale_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".debug");
        std::fs::write(&file, b"{}").unwrap();
        // 把修改时间拨回 2 小时前
        let f = std::fs::File::options().write(true).open(&file).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(7200))
            .unwrap();
        let mode = TestMode::new(file, 3600);
        assert!(!mode.active(), "超时残留文件不应阻断生产写入");
    }

    #[test]
    fn test_directory_is_not_active() {
        let dir = tempfile::tempdir().unwrap();
        let mode = TestMode::new(dir.path().to_path_buf(), 3600);
        assert!(!mode.active());
    }

    #[test]
    fn test_write_action_classification() {
        assert!(is_write_action(&RequestAction::Add));
        assert!(is_write_action(&RequestAction::Delete));
        assert!(is_write_action(&RequestAction::Dream));
        assert!(is_write_action(&RequestAction::SceneAddMemory));
        assert!(!is_write_action(&RequestAction::SearchMemory));
        assert!(!is_write_action(&RequestAction::Get));
        assert!(!is_write_action(&RequestAction::Stats));
        assert!(!is_write_action(&RequestAction::DreamReport));
    }
}
