//! # 服务端模块
//!
//! 提供 Unix Socket 服务器和 JSON-RPC 请求路由。
//!
//! - [`Router`]：请求路由器，将 JSON-RPC 方法分发到对应处理器
//! - [`UnixSocketServer`]：Unix 域套接字服务器，接受连接并处理请求

mod handler;
mod test_mode;
mod unix_socket;

pub use handler::RetrievalGraph;
pub use handler::Router;
pub use test_mode::{is_write_action, ConnMode, TestMode};
pub use unix_socket::UnixSocketServer;
