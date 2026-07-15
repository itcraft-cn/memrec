//! # 客户端模块
//!
//! 封装与 memrecd 守护进程的 Unix Socket 通信。

mod connection;

pub use connection::Client;
