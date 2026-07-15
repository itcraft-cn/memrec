//! # 项目 ID 检测
//!
//! 通过 `.mr_pid` 文件实现项目隔离，每个项目拥有唯一 UUID，
//! 确保不同项目的记忆互不干扰。

pub mod detect;

pub use detect::{detect_project_id, find_project_root, ProjectIdFile};
