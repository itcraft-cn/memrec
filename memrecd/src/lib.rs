//! # memrecd — MemRec 守护进程库
//!
//! 提供记忆存储、语义搜索、项目隔离等核心服务，
//! 通过 Unix Socket + JSON-RPC 2.0 对外暴露接口。
//!
//! ## 模块组织
//!
//! - [`config`]：守护进程配置加载与路径展开
//! - [`daemon`]：守护进程主循环，协调各子系统启动与关闭
//! - [`embedding`]：嵌入向量生成（基于 FastEmbed/ONNX Runtime）
//! - [`importance`]：记忆重要性评分计算
//! - [`lifecycle`]：记忆生命周期管理（软删除、硬删除、重要性重算）
//! - [`project`]：项目 ID 检测与 `.mr_pid` 文件管理
//! - [`server`]：Unix Socket 服务器与请求路由
//! - [`storage`]：RocksDB 存储层与向量存储

pub mod config;
pub mod daemon;
pub mod embedding;
pub mod importance;
pub mod lifecycle;
pub mod project;
pub mod server;
pub mod storage;
