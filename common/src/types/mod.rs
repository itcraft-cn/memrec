//! # 核心数据模型
//!
//! 定义 memrec 系统的核心数据类型，被所有下游 crate 共享使用。
//!
//! ## 模块组织
//!
//! - [`config`]：系统配置结构，包含记忆管理策略、重要性权重、服务端路径
//! - [`memory`]：记忆实体与记忆类型枚举，系统的核心数据单元
//! - [`model`]：嵌入模型抽象，支持多模型切换（MiniLM-L6-v2 / BGE-M3 / 自定义）
//! - [`project`]：项目隔离模型，实现多项目独立记忆空间

mod config;
mod memory;
mod model;
mod project;

pub use config::{ImportanceConfig, MemoryConfig, ServerConfig};
pub use memory::{Memory, MemoryType, MemorySource, MemoryScope};
pub use model::{ModelConfig, ModelFile, ModelFileType, ModelType, PoolingStrategy};
pub use project::{Project, ProjectConfig};
