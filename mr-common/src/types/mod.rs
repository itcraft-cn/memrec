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
//! - [`scene`]：场景聚合层，组织同主题的记忆卡片（L2 层）

mod config;
mod dedup;
mod dream_run;
mod edge;
mod facet;
mod llm;
mod memory;
mod model;
mod project;
mod propagation;
mod rule;
mod scene;
mod tiered;

pub use config::{
    DreamConfig, IdleMonitorConfig, ImportanceConfig, LogConfig, MemoryConfig, QueueConfig,
    SearchConfig, ServerConfig, SourceWeights,
};
pub use dedup::{DedupConfig, SimilarMemory};
pub use dream_run::{
    DreamRun, DreamRunStatus, GateOutcome, PhaseResultRecord, QualityMetrics, SummaryClues,
};
pub use edge::{EdgeType, MemoryEdge};
pub use facet::MemoryFacet;
pub use llm::{LlmConfig, ThinkLevel};
pub use memory::{Memory, MemoryScope, MemorySource, MemoryType};
pub use model::{ModelConfig, ModelFile, ModelFileType, ModelType, PoolingStrategy};
pub use project::{Project, ProjectConfig};
pub use propagation::{DeltaType, PropagationConfig, PropagationDelta};
pub use rule::{BoostType, RetrievalRule, RuleAction, RuleCondition, RuleStats, TimeRange};
pub use scene::Scene;
pub use tiered::{EdgeHint, RepresentationTier, TierThresholds, TieredMemory, TieredSearchResult};
