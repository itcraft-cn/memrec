//! # CLI 命令实现
//!
//! 各子命令的具体逻辑，通过 [`Client`] 与守护进程交互。

mod memory;
mod scene;
mod search;

pub use memory::{add, delete, dream, get, list, stats, version, AddOptions};
pub use scene::{add_memory, create, delete as scene_delete, get as scene_get, list as scene_list, remove_memory, update_heat};
pub use search::{search_execute, SearchArgs};
