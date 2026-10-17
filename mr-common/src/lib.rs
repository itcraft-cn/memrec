//! # mr-common 共享类型库
//!
//! 本 crate 为 memrec 生态提供共享的数据类型定义。

pub mod types;

pub use types::{
    BoostType, DeltaType, DreamRun, DreamRunStatus, EdgeHint, EdgeType, GateOutcome, Memory,
    MemoryEdge, MemoryFacet, MemoryScope, MemorySource, MemoryType, PhaseResultRecord, Project,
    ProjectConfig, PropagationConfig, PropagationDelta, QualityMetrics, RepresentationTier,
    RetrievalRule, RuleAction, RuleCondition, RuleStats, Scene, SummaryClues, TierThresholds,
    TieredMemory, TieredSearchResult, TimeRange,
};
pub use types::{
    DedupConfig, IdleMonitorConfig, LlmConfig, LogConfig, QueueConfig, SimilarMemory, ThinkLevel,
};
pub use types::{
    DreamConfig, ImportanceConfig, MemoryConfig, SearchConfig, ServerConfig, SourceWeights,
};
pub use types::{ModelConfig, ModelFile, ModelFileType, ModelType, PoolingStrategy};
