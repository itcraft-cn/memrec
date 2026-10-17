//! # DreamRun 审计记录
//!
//! 定义 [`DreamRun`] 及其配套的枚举与度量结构，作为 Dream 两段式管线
//! （Stage F 融合 + Stage N 成网）的治理载体（D2）。
//!
//! ## 定位
//!
//! - 一次 Dream 执行对应一条 `DreamRun`：运行开始写入占位（[`DreamRunStatus::Running`]），
//!   结束更新为终态（`Success` / `Failed` / `NoOp`）；
//! - **失败也落库**：与 `dream.state`（`last_run_at`，失败不刷新）不同，
//!   DreamRun 记录失败以便诊断与重试；
//! - 全部能力（查询、预演对比、度量、失败诊断）都挂在 DreamRun 上。
//!
//! ## 命名正交
//!
//! 本模块不复用 `Scene`；新概念一律用 `DreamRun` / `LlmEdgeGenerator` /
//! `EdgeBuilder` / `DreamReport`。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Dream 摘要中的关键数据线索集合。
///
/// 用于质量度量（线索保留率）与可追溯性。由摘要文本抽取得到。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryClues {
    /// 数字数据（含单位），如 `75万条`、`4.2KB`
    pub numbers: Vec<String>,
    /// 文件路径，如 `xnet_handler.rs:136`
    pub file_paths: Vec<String>,
    /// 行号，如 `:136`
    pub line_numbers: Vec<u32>,
    /// 来源线索类型，如 `log-based`、`gdb-analysis`
    pub source_clues: HashSet<String>,
    /// 内存相关术语，如 `Eden`、`RSS`
    pub memory_terms: HashSet<String>,
}

impl SummaryClues {
    /// 线索总量（用于线索保留率分母）。
    pub fn total(&self) -> usize {
        self.numbers.len()
            + self.file_paths.len()
            + self.line_numbers.len()
            + self.source_clues.len()
            + self.memory_terms.len()
    }
}

/// Dream 门控结果（记录"为何允许/为何未执行"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateOutcome {
    /// 通过门控，正常执行
    Allowed,
    /// Dream 未启用
    Disabled,
    /// 距上次执行过近
    TooSoon,
    /// 新增记忆不足
    NotEnoughNew,
    /// 需要 LLM 但未配置
    LlmNotConfigured,
}

impl Default for GateOutcome {
    fn default() -> Self {
        GateOutcome::Allowed
    }
}

/// DreamRun 终态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamRunStatus {
    /// 运行中（占位，结束时更新）
    Running,
    /// 成功执行（含"无事可做"的 NoOp 之外的正常完成）
    Success,
    /// 执行失败
    Failed,
    /// 成功执行但无候选（无事可做）
    NoOp,
}

impl Default for DreamRunStatus {
    fn default() -> Self {
        DreamRunStatus::Running
    }
}

/// 单个阶段的执行记录（DreamRun 内的可序列化快照）。
///
/// 与 `mr-ability::dream::PhaseResult` 字段一致，便于审计落库。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseResultRecord {
    pub phase: String,
    pub success: bool,
    pub processed_count: usize,
    pub created_count: usize,
    pub error_message: Option<String>,
}

/// DreamRun 质量度量。
///
/// 全部为运行期内在指标，与离线评测基准（Eval Harness）互补。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityMetrics {
    /// 线索保留率：摘要保留线索数 / 输入线索数（0.0-1.0）
    pub clue_retention: f32,
    /// 压缩比：源总字数 / 产出总字数（越大压缩越强）
    pub compression_ratio: f32,
    /// 平均簇大小
    pub avg_cluster_size: f32,
    /// 识别出的矛盾对数
    pub contradiction_count: usize,
    /// 新建边数
    pub edge_count: usize,
    /// 传播增量数
    pub propagation_count: usize,
    /// 阶段成功率（0.0-1.0）
    pub phase_success_rate: f32,
}

impl Default for QualityMetrics {
    fn default() -> Self {
        Self {
            clue_retention: 0.0,
            compression_ratio: 0.0,
            avg_cluster_size: 0.0,
            contradiction_count: 0,
            edge_count: 0,
            propagation_count: 0,
            phase_success_rate: 0.0,
        }
    }
}

/// 一次 Dream 运行的审计记录。
///
/// 覆盖 D2 的全部治理能力：执行结果、产出指纹、边/传播活跃度、LLM 用量与质量度量。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamRun {
    /// 运行标识
    pub run_id: Uuid,
    /// 开始时间
    pub started_at: DateTime<Utc>,
    /// 结束时间（运行中为 None）
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
    /// 耗时（毫秒）
    #[serde(default)]
    pub duration_ms: u64,
    /// 是否强制触发
    #[serde(default)]
    pub force: bool,
    /// 是否为预演（dry-run 不写库、不删数据）
    #[serde(default)]
    pub dry_run: bool,
    /// 门控结果
    #[serde(default)]
    pub gate_outcome: GateOutcome,
    /// 运行终态
    #[serde(default)]
    pub status: DreamRunStatus,
    /// 失败信息
    #[serde(default)]
    pub error: Option<String>,
    /// 参与候选数
    #[serde(default)]
    pub candidate_count: usize,
    /// 聚类簇数
    #[serde(default)]
    pub cluster_count: usize,
    /// 本次写入的整合记忆（含事实条与摘要）
    #[serde(default)]
    pub integrated_memory_ids: Vec<Uuid>,
    /// 主题摘要记忆
    #[serde(default)]
    pub summary_memory_id: Option<Uuid>,
    /// 本次软删的源
    #[serde(default)]
    pub deleted_source_ids: Vec<Uuid>,
    /// 各阶段执行结果
    #[serde(default)]
    pub phase_results: Vec<PhaseResultRecord>,
    /// 本次新建边数量
    #[serde(default)]
    pub edge_count: usize,
    /// 本次新建边按类型分布（类型名 → 数量）
    #[serde(default)]
    pub edge_counts_by_type: HashMap<String, usize>,
    /// 本次传播增量数
    #[serde(default)]
    pub propagation_count: usize,
    /// 识别出的矛盾对数
    #[serde(default)]
    pub contradiction_count: usize,
    /// LLM 调用次数
    #[serde(default)]
    pub llm_calls: u64,
    /// LLM 消耗 token 数（如可得）
    #[serde(default)]
    pub llm_tokens: u64,
    /// LLM 累计延迟（毫秒）
    #[serde(default)]
    pub llm_latency_ms: u64,
    /// 摘要中的关键数据线
    #[serde(default)]
    pub clues: SummaryClues,
    /// 质量度量
    #[serde(default)]
    pub metrics: QualityMetrics,
}

impl DreamRun {
    /// 创建运行中占位记录。
    pub fn running(force: bool, dry_run: bool) -> Self {
        Self {
            run_id: Uuid::new_v4(),
            started_at: Utc::now(),
            finished_at: None,
            duration_ms: 0,
            force,
            dry_run,
            gate_outcome: GateOutcome::Allowed,
            status: DreamRunStatus::Running,
            error: None,
            candidate_count: 0,
            cluster_count: 0,
            integrated_memory_ids: Vec::new(),
            summary_memory_id: None,
            deleted_source_ids: Vec::new(),
            phase_results: Vec::new(),
            edge_count: 0,
            edge_counts_by_type: HashMap::new(),
            propagation_count: 0,
            contradiction_count: 0,
            llm_calls: 0,
            llm_tokens: 0,
            llm_latency_ms: 0,
            clues: SummaryClues::default(),
            metrics: QualityMetrics::default(),
        }
    }

    /// 标记为终态并补齐耗时。
    pub fn finish(&mut self, status: DreamRunStatus) {
        self.status = status;
        let finished = Utc::now();
        self.duration_ms = (finished - self.started_at)
            .num_milliseconds()
            .max(0) as u64;
        self.finished_at = Some(finished);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dream_run_running_placeholder() {
        let run = DreamRun::running(true, false);
        assert!(run.force);
        assert!(!run.dry_run);
        assert_eq!(run.status, DreamRunStatus::Running);
        assert!(run.finished_at.is_none());
    }

    #[test]
    fn test_dream_run_finish_sets_duration() {
        let mut run = DreamRun::running(false, false);
        run.finish(DreamRunStatus::Success);
        assert_eq!(run.status, DreamRunStatus::Success);
        assert!(run.finished_at.is_some());
    }

    #[test]
    fn test_dream_run_serde_roundtrip() {
        let mut run = DreamRun::running(false, true);
        run.candidate_count = 12;
        run.cluster_count = 3;
        run.metrics.clue_retention = 0.75;
        let json = serde_json::to_string(&run).unwrap();
        let parsed: DreamRun = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.run_id, run.run_id);
        assert_eq!(parsed.candidate_count, 12);
        assert_eq!(parsed.metrics.clue_retention, 0.75);
    }

    #[test]
    fn test_dream_run_backward_compat_minimal_json() {
        // 仅提供 run_id/started_at 也应可反序列化（其余走 default）
        let json = format!(
            r#"{{"run_id":"{}","started_at":"2026-10-17T00:00:00Z"}}"#,
            Uuid::nil()
        );
        let parsed: DreamRun = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, DreamRunStatus::Running);
        assert_eq!(parsed.edge_count, 0);
    }

    #[test]
    fn test_summary_clues_total() {
        let mut clues = SummaryClues::default();
        clues.numbers.push("4.2KB".to_string());
        clues.line_numbers.push(136);
        clues.source_clues.insert("log-based".to_string());
        assert_eq!(clues.total(), 3);
    }
}
