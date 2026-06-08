pub mod dirs;
pub mod config;
pub mod download;
pub mod service;
pub mod verify;

#[cfg(target_os = "linux")]
pub mod systemd;

#[cfg(target_os = "macos")]
pub mod launchd;

#[cfg(target_os = "windows")]
pub mod windows_service;

pub use dirs::create_directories;
pub use config::generate_config;
pub use download::download_model;
pub use download::DownloadOptions;
pub use service::ServiceManager;
pub use service::detect_service_manager;
pub use verify::run_verification;