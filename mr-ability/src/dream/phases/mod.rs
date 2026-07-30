//! # Dream 阶段处理模块
//!
//! 包含4个独立的阶段处理器：
//! 1. CrossProjectExtract: 交叉项目记忆提取
//! 2. PersonalSummary: 个人总体记忆提炼
//! 3. Cleanup: 记忆清理
//! 4. VectorRegen: 向量重新生成

mod cross_project;
mod personal_summary;
mod cleanup;
mod vector_regen;

pub use cross_project::CrossProjectExtractor;
pub use personal_summary::PersonalSummarizer;
pub use cleanup::MemoryCleaner;
pub use vector_regen::VectorRegenerator;

/// 阶段执行结果。
#[derive(Debug, Clone)]
pub struct PhaseResult {
    pub phase: String,
    pub success: bool,
    pub processed_count: usize,
    pub created_count: usize,
    pub error_message: Option<String>,
}

impl PhaseResult {
    pub fn ok(phase: &str, processed: usize, created: usize) -> Self {
        Self {
            phase: phase.to_string(),
            success: true,
            processed_count: processed,
            created_count: created,
            error_message: None,
        }
    }

    pub fn err(phase: &str, message: String) -> Self {
        Self {
            phase: phase.to_string(),
            success: false,
            processed_count: 0,
            created_count: 0,
            error_message: Some(message),
        }
    }
}