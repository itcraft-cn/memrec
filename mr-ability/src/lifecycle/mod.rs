//! # 记忆生命周期管理
//!
//! 负责记忆的软删除清理、硬删除和重要性重算，
//! 确保存储空间合理使用。

mod manager;

pub use manager::LifecycleManager;
