//! # mr-common 共享类型库
//!
//! 本 crate 为 memrec 生态提供共享的数据类型定义。

pub mod types;

pub use types::{
    BoostType, DeltaType, EdgeHint, EdgeType, Memory, MemoryEdge, MemoryFacet, MemoryScope,
    MemorySource, MemoryType, Project, ProjectConfig, PropagationConfig, PropagationDelta,
    RepresentationTier, RetrievalRule, RuleAction, RuleCondition, RuleStats, Scene, TierThresholds,
    TieredMemory, TieredSearchResult, TimeRange,
};
pub use types::{
    DedupConfig, IdleMonitorConfig, LlmConfig, LogConfig, QueueConfig, SimilarMemory, ThinkLevel,
};
pub use types::{
    DreamConfig, ImportanceConfig, MemoryConfig, SearchConfig, ServerConfig, SourceWeights,
};
pub use types::{ModelConfig, ModelFile, ModelFileType, ModelType, PoolingStrategy};
