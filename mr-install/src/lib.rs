pub mod dirs;
pub mod config;
pub mod download;
pub mod systemd;
pub mod verify;

pub use dirs::create_directories;
pub use config::generate_config;
pub use download::download_model;
pub use systemd::register_service;
pub use verify::run_verification;
