//! # memrecd — MemRec 守护进程库
//!
//! 提供守护进程启动、配置管理、Unix Socket 服务器等，
//! 核心能力委托给 mr-ability。

pub mod config;
pub mod daemon;
pub mod server;