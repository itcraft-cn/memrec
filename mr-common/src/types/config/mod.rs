//! # 系统配置类型
//!
//! 定义记忆管理策略、重要性计算权重、搜索配置、服务端路径等配置结构。
//!
//! ## 配置层次
//!
//! - [`MemoryConfig`]：记忆生命周期管理策略（软删除恢复期、硬删除阈值、存储水位线等）
//! - [`ImportanceConfig`]：重要性计算公式的权重参数（时效性/频率/语义/显式评分）
//! - [`SearchConfig`]：搜索行为配置（混合检索权重、MMR 重排、时间衰减、来源权重）
//! - [`ServerConfig`]：服务端路径配置（Unix Socket 路径、数据目录）
//!
//! 这些配置通过 `config.toml` 持久化，由 [`memrecd::config::DaemonConfig`] 加载。


mod dream;
mod memory;
mod monitor;
mod search;
mod server;

pub use dream::DreamConfig;
pub use memory::{ImportanceConfig, MemoryConfig, SourceWeights};
pub use monitor::{IdleMonitorConfig, QueueConfig};
pub use search::SearchConfig;
pub use server::{LogConfig, ServerConfig};

#[cfg(test)]
mod tests;
