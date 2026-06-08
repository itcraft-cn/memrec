pub mod dirs;
pub mod config;
pub mod download;
pub mod install;
pub mod service;
pub mod verify;

#[cfg(target_os = "linux")]
pub mod systemd;

#[cfg(target_os = "macos")]
pub mod launchd;

pub use dirs::create_directories;
pub use dirs::default_bin_dir;
pub use config::generate_config;
pub use download::download_model;
pub use download::DownloadOptions;
pub use install::install_binaries;
pub use install::InstallOptions;
pub use service::ServiceManager;
pub use service::detect_service_manager;
pub use verify::run_verification;