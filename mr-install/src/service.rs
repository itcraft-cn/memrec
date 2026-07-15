use anyhow::Result;
use std::path::Path;

pub trait ServiceManager {
    fn name(&self) -> &str;

    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()>;

    fn start(&self) -> Result<()>;

    fn stop(&self) -> Result<()>;

    fn is_active(&self) -> bool;

    fn unregister(&self) -> Result<()>;
}

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
