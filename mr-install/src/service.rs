//! # 服务管理
//!
//! 定义 [`ServiceManager`] trait，根据平台自动检测 systemd/launchd。

use anyhow::Result;
use std::path::Path;

/// 服务管理器 trait，统一 systemd/launchd 操作接口
pub trait ServiceManager {
    /// 服务管理器名称（"systemd" 或 "launchd"）
    fn name(&self) -> &str;

    /// 注册服务
    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()>;

    /// 启动服务
    fn start(&self) -> Result<()>;

    /// 停止服务
    fn stop(&self) -> Result<()>;

    /// 检查服务是否运行中
    fn is_active(&self) -> bool;

    /// 注销服务
    fn unregister(&self) -> Result<()>;
}

/// 根据当前平台检测服务管理器
pub fn detect_service_manager() -> Box<dyn ServiceManager> {
    #[cfg(target_os = "linux")]
    {
        Box::new(crate::systemd::SystemdService)
    }

    #[cfg(target_os = "macos")]
    {
        Box::new(crate::launchd::LaunchdService)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        compile_error!("Unsupported platform. Only Linux and macOS are supported.");
    }
}
