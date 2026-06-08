use anyhow::Result;

pub trait ServiceManager {
    fn name(&self) -> &str;
    
    fn register(&self, bin_path: &std::path::Path, home_dir: &std::path::Path) -> Result<()>;
    
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
    
    #[cfg(target_os = "windows")]
    {
        Box::new(crate::windows_service::WindowsService)
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        compile_error!("Unsupported platform. Only Linux, macOS, and Windows are supported.");
    }
}
