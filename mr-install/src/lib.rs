//! # mr-install — MemRec 安装器库
//!
//! 提供二进制安装、模型下载、配置生成、服务注册、安装验证等功能。
//!
//! ## 模块
//!
//! - `config`：安装配置生成（TOML）
//! - `dirs`：目录创建（`~/.memrec/`）
//! - `download`：模型文件下载（HuggingFace/镜像）
//! - `install`：二进制安装（cargo install）
//! - `service`：服务管理 trait + 平台检测
//! - `systemd`：Linux systemd 用户服务
//! - `launchd`：macOS LaunchAgent 服务
//! - `verify`：安装后验证

pub mod config;
pub mod dirs;
pub mod download;
pub mod install;
pub mod service;
pub mod verify;

#[cfg(target_os = "linux")]
pub mod systemd;

#[cfg(target_os = "macos")]
pub mod launchd;

pub use config::generate_config;
pub use dirs::create_directories;
pub use dirs::default_bin_dir;
pub use download::download_model;
pub use download::DownloadOptions;
pub use install::install_binaries;
pub use install::InstallOptions;
pub use service::detect_service_manager;
pub use service::ServiceManager;
pub use verify::run_verification;
