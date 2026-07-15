//! # CLI 命令实现
//!
//! 各子命令的具体逻辑，通过 [`Client`] 与守护进程交互。

mod memory;
mod search;

pub use memory::{add, delete, get, list, stats, version};
pub use search::{search_execute, SearchArgs};
