//! # JSON-RPC 2.0 响应定义
//!
//! 定义守护进程返回给客户端的所有响应类型，包括：
//!
//! - [`JsonRpcResponse`]：JSON-RPC 2.0 标准响应包装，成功时携带 [`ResponseResult`]，失败时携带 [`JsonRpcError`]
//! - [`ResponseResult`]：响应结果联合类型，使用内部标签区分各操作返回值
//! - 各操作的具体返回结构：[`MemoryResult`]、[`SemanticSearchResult`]、[`StatsResult`] 等
//!
//! ## 设计要点
//!
//! - [`SearchHit`] 包含记忆摘要预览（`content_preview`）和分块信息，避免大内容直接传输
//! - [`SemanticSearchResult`] 分别记录嵌入生成耗时与搜索耗时，便于性能分析
//! - [`SuccessResult`] 实现了 `From<bool>` 便于从布尔值构造成功/失败消息

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::JsonRpcError;
use mr_common::{
    DreamRun, DreamRunStatus, GateOutcome, Memory, MemoryScope, MemorySource, MemoryType,
    PhaseResultRecord, Project, QualityMetrics, Scene, SimilarMemory, SummaryClues,
    TieredSearchResult,
};

/// JSON-RPC 2.0 标准响应对象。
///
/// 成功时 `result` 为 `Some`、`error` 为 `None`；失败时反之。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC 协议版本，固定为 `"2.0"`
    pub jsonrpc: String,
    /// 成功时的返回结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ResponseResult>,
    /// 失败时的错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// 请求 ID，与请求中的 `id` 对应
    pub id: u64,
}

impl JsonRpcResponse {
    /// 构造成功响应。
    pub fn success(result: ResponseResult, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// 构造错误响应。
    pub fn error(err: JsonRpcError, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(err),
            id,
        }
    }
}

/// 响应结果联合类型。
///
/// 使用 `#[serde(tag = "type")]` 内部标签序列化，客户端可根据 `type` 字段区分结果类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseResult {
    Memory(MemoryResult),
    MemoryList(MemoryListResult),
    SearchResult(SearchResult),
    SemanticSearchResult(SemanticSearchResult),
    TieredSearchResult(TieredSearchResult),
    Project(ProjectResult),
    ProjectList(ProjectListResult),
    ProjectInfo(ProjectInfoResult),
    Version(VersionResult),
    Dream(DreamResult),
    DreamReport(DreamReportResult),
    Config(ConfigResult),
    Stats(StatsResult),
    Success(SuccessResult),
    Scene(SceneResult),
    SceneList(SceneListResult),
}

/// 对外暴露的记忆视图。
///
/// 这是 JSON-RPC 协议中唯一允许出现的记忆结构：`Memory` 作为领域模型兼
/// 持久化结构，包含 `embedding`、`last_accessed`、`access_count`、`metadata`
/// 等内部实现字段，这些字段对调用方没有判断价值，且 `embedding` 在不同写入
/// 路径下可能为 `None` 或上千维数组，容易被误读为"是否已建索引"的状态位。
///
/// 因此协议层与存储层解耦：存储仍使用 `Memory`，响应统一转换为
/// [`MemoryView`]，只保留调用方判断内容与归属所需的字段。
///
/// 可选字段在无值时直接省略（不输出 `null`），避免响应中出现无意义占位。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryView {
    pub id: Uuid,
    pub memory_type: MemoryType,
    /// 记忆正文；分块记忆为当前分块内容
    pub content: String,
    /// 摘要，仅当记忆被 Dream 整合等流程生成摘要时存在
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// 重要性（0-1）
    pub importance: f32,
    pub created_at: DateTime<Utc>,
    pub tags: Vec<String>,
    /// 所属项目；`None` 表示全局记忆
    pub project_id: Option<Uuid>,
    /// 是否已软删除；`mr_get` 等读取路径需要据此排除无效证据
    pub is_deleted: bool,
    /// 软删除时间，仅已删除记录返回
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    /// 分块组 ID，仅分块记忆返回
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_group_id: Option<Uuid>,
    /// 分块序号（从 0 开始），仅分块记忆返回
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<u32>,
    /// 分块总数，仅分块记忆返回
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_total: Option<u32>,
    /// 记忆来源
    pub source: MemorySource,
    /// 作用域（项目/全局）
    pub scope: MemoryScope,
}

impl From<&Memory> for MemoryView {
    fn from(memory: &Memory) -> Self {
        Self {
            id: memory.id,
            memory_type: memory.memory_type,
            content: memory.content.clone(),
            summary: memory.summary.clone(),
            importance: memory.importance,
            created_at: memory.created_at,
            tags: memory.tags.clone(),
            project_id: memory.project_id,
            is_deleted: memory.is_deleted,
            deleted_at: memory.deleted_at,
            chunk_group_id: memory.chunk_group_id,
            chunk_index: memory.chunk_index,
            chunk_total: memory.chunk_total,
            source: memory.source,
            scope: memory.scope,
        }
    }
}

impl From<Memory> for MemoryView {
    fn from(memory: Memory) -> Self {
        Self::from(&memory)
    }
}

/// 单条记忆操作结果，用于 Add/Get/Update 等操作。
///
/// `action` 描述写入决策：`added`（正常新增）/ `skipped`（重复跳过）/ `updated`（合并更新）；
/// `duplicates` 携带命中的相似记忆候选（去重检测开启时）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub memory: MemoryView,
    /// 写入动作：`added` / `skipped` / `updated`
    #[serde(default = "default_add_action")]
    pub action: String,
    /// 相似记忆候选（相似度降序）
    #[serde(default)]
    pub duplicates: Vec<SimilarMemory>,
}

fn default_add_action() -> String {
    "added".to_string()
}

impl MemoryResult {
    /// 构造默认动作（`added`）的结果。
    pub fn new(memory: Memory) -> Self {
        Self {
            memory: memory.into(),
            action: default_add_action(),
            duplicates: Vec::new(),
        }
    }
}

/// 记忆列表结果，用于 List 操作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListResult {
    /// 匹配的记忆列表
    pub memories: Vec<MemoryView>,
    /// 总匹配数（可能大于返回数量）
    pub total: usize,
}

/// 精确搜索结果，用于 Search 操作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memories: Vec<MemoryView>,
    pub total: usize,
    /// 搜索耗时（毫秒）
    pub elapsed_ms: u64,
}

/// 语义搜索结果，用于 SearchMemory 操作。
///
/// 结果以 [`SearchHit`] 摘要形式返回，不含完整内容，减少传输开销。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    /// 匹配的记忆摘要列表，按相似度降序排列
    pub results: Vec<SearchHit>,
    pub total: usize,
    /// 嵌入向量生成耗时（毫秒）
    pub query_embedding_time_ms: u64,
    /// 向量搜索耗时（毫秒）
    pub search_time_ms: u64,
}

/// 语义搜索命中摘要。
///
/// 包含记忆的元信息和内容预览，不含完整 `content` 和 `embedding`，
/// 客户端可通过 `memory_id` 调用 Get 获取完整内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub memory_id: Uuid,
    /// 校准后的相关度（0-1）。向量通路取原始余弦，全文通路取查询词覆盖率，
    /// 双路命中按 noisy-OR 合成；`min_score` 作用于此值。
    pub score: f32,
    pub memory_type: MemoryType,
    /// 内容前若干字符的预览
    pub content_preview: String,
    pub project_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub is_chunked: bool,
    pub chunk_group_id: Option<Uuid>,
    pub chunk_index: Option<u32>,
    pub chunk_total: Option<u32>,
    pub created_at: DateTime<Utc>,
    /// 经图扩散到达时的边提示（None 表示直接命中）
    #[serde(default)]
    pub edge_hint: Option<mr_common::EdgeHint>,
    /// 原始向量余弦相似度（仅向量通路命中时存在）。
    #[serde(default)]
    pub vector_score: Option<f32>,
    /// 原始 BM25 分（仅全文通路命中时存在，量纲与余弦不可比）。
    #[serde(default)]
    pub fts_score: Option<f32>,
    /// 查询词覆盖率（0-1，仅全文通路命中时存在）。
    #[serde(default)]
    pub coverage: Option<f32>,
    /// 加权 RRF 名次分（0-1），用于同分裁决与可解释性。
    #[serde(default)]
    pub rank_score: Option<f32>,
}

/// 单个项目操作结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResult {
    pub project: Project,
}

/// 项目列表结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectListResult {
    pub projects: Vec<Project>,
    pub total: usize,
}

/// 当前项目信息结果，由 GetProjectInfo 操作返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfoResult {
    pub project_id: Uuid,
    pub project_name: Option<String>,
    /// 项目根目录路径
    pub project_root: String,
    /// 该项目下的记忆数量
    pub memory_count: usize,
    /// `.mr_pid` 文件是否存在
    pub mr_pid_exists: bool,
}

/// 版本查询结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResult {
    pub version: String,
}

/// Dream 记忆整合结果。
///
/// 既有字段（`integrated_count` / `created_memory_id` / `summary`）保持兼容，
/// 新增字段用于 DreamReport 治理与 dry-run 展示。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamResult {
    /// 历史字段：本次输入的源记忆数（等价于 `input_count`）
    pub integrated_count: usize,
    /// 本次输入的源记忆数
    #[serde(default)]
    pub input_count: usize,
    /// 本次实际产出的记忆数（facts + summaries）
    #[serde(default)]
    pub produced_count: usize,
    pub created_memory_id: Option<Uuid>,
    pub summary: String,
    /// 本次运行审计 ID
    #[serde(default)]
    pub run_id: Option<Uuid>,
    /// 聚类簇数
    #[serde(default)]
    pub cluster_count: usize,
    /// 矛盾对数
    #[serde(default)]
    pub contradiction_count: usize,
    /// 新建边数
    #[serde(default)]
    pub edge_count: usize,
    /// 传播增量数
    #[serde(default)]
    pub propagation_count: usize,
    /// 是否预演
    #[serde(default)]
    pub dry_run: bool,
    /// 阶段执行结果
    #[serde(default)]
    pub phase_results: Vec<PhaseResultRecord>,
    /// 摘要中的关键数据线索
    #[serde(default)]
    pub clues: SummaryClues,
    /// 质量度量
    #[serde(default)]
    pub metrics: QualityMetrics,
}

/// Dream 运行审计报告结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamReportResult {
    /// 最近若干条 DreamRun（按开始时间倒序）
    pub runs: Vec<DreamRun>,
    /// 运行总数
    pub total: usize,
}

/// 配置查询/设置结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResult {
    pub key: String,
    pub value: String,
}

/// 统计信息结果，由 Stats 操作返回。
///
/// 仅保留可被真实统计的计数项。曾有的 `storage_usage`、`avg_importance` 因缺乏可靠
/// 数据来源（无容量配额映射、需全量扫描）而长期硬编码为 0.0，属虚假精度，已移除；
/// 如后续需要，应基于真实数据源（RocksDB live data size、重要性二级索引）再引入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub total_memories: usize,
    pub active_memories: usize,
    pub deleted_memories: usize,
    /// Dream/raw 构成与累计 Dream 运行统计；未注入 DreamRun 存储时为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dream: Option<DreamStats>,
}

/// 活跃记忆构成与累计 Dream 运行统计。
///
/// `active_raw` / `active_dream` 基于活跃记忆扫描：Dream 产出由
/// [`mr_common::Memory::is_dream_generated`] 判定（融合产出的 `source_ids`
/// 或 Dream 阶段摘要标签）。`*_runs` 与累计量来自 `DreamRun` 审计记录。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DreamStats {
    /// 活跃记忆中的原始记录数（非 Dream 产出）。
    pub active_raw: usize,
    /// 活跃记忆中的 Dream 产出数。
    pub active_dream: usize,
    /// 原始记录占活跃记忆比例（0-1）。
    pub raw_ratio: f32,
    /// Dream 产出占活跃记忆比例（0-1）。
    pub dream_ratio: f32,
    /// 最近一次 Dream 记录时间（优先 `finished_at`，回退 `started_at`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<DateTime<Utc>>,
    /// 最近一次运行状态（running/success/failed/no_op/dry_run）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_status: Option<String>,
    /// 最近一次记录是否为 dry-run。
    #[serde(default)]
    pub last_run_dry_run: bool,
    /// 最近一次门控结果（allowed/disabled/too_soon/not_enough_new/llm_not_configured）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_gate: Option<String>,
    /// 最近一次成功整合时间。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
    /// 累计记录数（含门控跳过、失败、dry-run）。
    pub total_runs: usize,
    /// 实际执行数（通过门控；含成功/失败/无候选）。
    pub actual_runs: usize,
    pub success_runs: usize,
    pub failed_runs: usize,
    /// 终态为 no_op 的记录数（含门控跳过）。
    pub noop_runs: usize,
    /// 因门控未执行（gate_outcome != allowed）的记录数。
    pub gate_skipped_runs: usize,
    pub dry_run_runs: usize,
    pub running_runs: usize,
    pub total_candidates: usize,
    pub total_clusters: usize,
    /// 累计输入源记忆数
    pub total_input: usize,
    /// 累计实际产出记忆数（facts + summaries）
    pub total_produced: usize,
    /// 历史字段：累计产出（等价于 total_produced）
    pub total_integrated: usize,
    /// 累计标记为已处理的源记忆数（不再参与后续 Dream）
    pub total_processed_sources: usize,
    /// 历史字段：旧版本软删的源记忆数
    pub total_deleted_sources: usize,
    pub total_edges: usize,
    pub total_propagations: usize,
    /// 累计 LLM 调用次数（当前管线未上报用量时恒为 0）。
    pub total_llm_calls: u64,
    /// 累计 LLM token 数（当前管线未上报用量时恒为 0）。
    pub total_llm_tokens: u64,
}

impl DreamStats {
    /// 由活跃记忆构成与 DreamRun 列表聚合统计。
    pub fn from_runs(active_raw: usize, active_dream: usize, runs: &[DreamRun]) -> Self {
        let active_total = active_raw + active_dream;
        let mut stats = Self {
            active_raw,
            active_dream,
            raw_ratio: ratio(active_raw, active_total),
            dream_ratio: ratio(active_dream, active_total),
            ..Default::default()
        };

        let mut last_run: Option<&DreamRun> = None;
        let mut last_success_at: Option<DateTime<Utc>> = None;

        for run in runs {
            stats.total_runs += 1;
            match run.status {
                DreamRunStatus::Success => {
                    if !run.dry_run {
                        stats.success_runs += 1;
                    }
                }
                DreamRunStatus::Failed => stats.failed_runs += 1,
                DreamRunStatus::NoOp => stats.noop_runs += 1,
                DreamRunStatus::Running => stats.running_runs += 1,
            }
            if run.gate_outcome != GateOutcome::Allowed {
                stats.gate_skipped_runs += 1;
            }
            if run.dry_run {
                stats.dry_run_runs += 1;
            }

            stats.total_candidates += run.candidate_count;
            stats.total_clusters += run.cluster_count;
            let produced = run.produced_count.max(run.integrated_memory_ids.len());
            stats.total_input += run.candidate_count;
            stats.total_produced += produced;
            stats.total_integrated += produced;
            stats.total_processed_sources += run.processed_source_ids.len();
            stats.total_deleted_sources += run.deleted_source_ids.len();
            stats.total_edges += run.edge_count;
            stats.total_propagations += run.propagation_count;
            stats.total_llm_calls += run.llm_calls;
            stats.total_llm_tokens += run.llm_tokens;

            let at = run_time(run);
            if last_run.is_none_or(|prev| at > run_time(prev)) {
                last_run = Some(run);
            }
            if run.status == DreamRunStatus::Success
                && !run.dry_run
                && last_success_at.is_none_or(|prev| at > prev)
            {
                last_success_at = Some(at);
            }
        }

        stats.actual_runs = stats.success_runs
            + stats.failed_runs
            + stats.noop_runs.saturating_sub(stats.gate_skipped_runs);
        if let Some(run) = last_run {
            stats.last_run_at = Some(run_time(run));
            stats.last_run_dry_run = run.dry_run;
            stats.last_run_status = Some(if run.dry_run {
                "dry_run".to_string()
            } else {
                run.status.as_str().to_string()
            });
            stats.last_run_gate = Some(run.gate_outcome.as_str().to_string());
        }
        stats.last_success_at = last_success_at;
        stats
    }
}

fn ratio(part: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        part as f32 / total as f32
    }
}

fn run_time(run: &DreamRun) -> DateTime<Utc> {
    run.finished_at.unwrap_or(run.started_at)
}

/// 通用成功/失败结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResult {
    pub message: String,
}

/// 从布尔值构造成功/失败消息。
///
/// `true` 映射为 `"Success"`，`false` 映射为 `"Failed"`。
impl From<bool> for SuccessResult {
    fn from(success: bool) -> Self {
        Self {
            message: if success {
                "Success".to_string()
            } else {
                "Failed".to_string()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_common::MemoryType;

    #[test]
    fn test_success_response() {
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        let resp = JsonRpcResponse::success(ResponseResult::Memory(MemoryResult::new(memory)), 1);

        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_error_response() {
        let err = JsonRpcError {
            code: -32001,
            message: "Not found".to_string(),
            data: None,
        };
        let resp = JsonRpcResponse::error(err, 1);

        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_response_serde() {
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        let resp = JsonRpcResponse::success(ResponseResult::Memory(MemoryResult::new(memory)), 1);

        let json = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(resp.id, parsed.id);
    }

    #[test]
    fn test_memory_view_omits_internal_fields() {
        let mut memory = Memory::new("test content".to_string(), MemoryType::Knowledge);
        memory.embedding = Some(vec![0.1, 0.2]);
        memory
            .metadata
            .insert("source_ids".to_string(), "x".to_string());

        let value = serde_json::to_value(MemoryView::from(&memory)).unwrap();

        // 内部字段不得出现在协议载荷中
        assert!(value.get("embedding").is_none());
        assert!(value.get("last_accessed").is_none());
        assert!(value.get("access_count").is_none());
        assert!(value.get("metadata").is_none());
        // 可选字段无值时省略，避免 null 占位
        assert!(value.get("summary").is_none());
        assert!(value.get("deleted_at").is_none());
        assert!(value.get("chunk_group_id").is_none());
        // 调用方判断内容与归属所需字段保留
        assert_eq!(value["content"], "test content");
        assert_eq!(value["source"], "user");
        assert_eq!(value["scope"], "project");
    }

    #[test]
    fn test_semantic_search_result_serde() {
        let result = SemanticSearchResult {
            results: vec![SearchHit {
                memory_id: Uuid::new_v4(),
                score: 0.95,
                memory_type: MemoryType::Decision,
                content_preview: "test content".to_string(),
                project_id: Some(Uuid::new_v4()),
                tags: vec!["critical".to_string()],
                is_chunked: false,
                chunk_group_id: None,
                chunk_index: None,
                chunk_total: None,
                created_at: Utc::now(),
                edge_hint: None,
                vector_score: Some(0.95),
                fts_score: None,
                coverage: None,
                rank_score: None,
            }],
            total: 1,
            query_embedding_time_ms: 10,
            search_time_ms: 5,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: SemanticSearchResult = serde_json::from_str(&json).unwrap();

        assert_eq!(result.total, parsed.total);
    }

    #[test]
    fn test_dream_stats_from_runs() {
        let mut success = DreamRun::running(true, false);
        success.started_at = Utc::now() - chrono::Duration::seconds(10);
        success.candidate_count = 10;
        success.cluster_count = 3;
        success.integrated_memory_ids = vec![Uuid::new_v4(), Uuid::new_v4()];
        success.produced_count = 2;
        success.processed_source_ids = vec![Uuid::new_v4(), Uuid::new_v4()];
        success.deleted_source_ids = vec![Uuid::new_v4()];
        success.edge_count = 4;
        success.propagation_count = 5;
        success.llm_calls = 2;
        success.llm_tokens = 300;
        success.finish(DreamRunStatus::Success);

        let mut skipped = DreamRun::running(false, false);
        skipped.started_at = Utc::now() - chrono::Duration::seconds(1);
        skipped.gate_outcome = GateOutcome::TooSoon;
        skipped.status = DreamRunStatus::NoOp;
        skipped.finish(DreamRunStatus::NoOp);

        let stats = DreamStats::from_runs(7, 3, &[skipped, success]);

        assert_eq!(stats.active_raw, 7);
        assert_eq!(stats.active_dream, 3);
        assert!((stats.dream_ratio - 0.3).abs() < 1e-6);
        assert!((stats.raw_ratio - 0.7).abs() < 1e-6);
        assert_eq!(stats.total_runs, 2);
        assert_eq!(stats.success_runs, 1);
        assert_eq!(stats.noop_runs, 1);
        assert_eq!(stats.gate_skipped_runs, 1);
        assert_eq!(stats.actual_runs, 1);
        assert_eq!(stats.total_integrated, 2);
        assert_eq!(stats.total_input, 10);
        assert_eq!(stats.total_produced, 2);
        assert_eq!(stats.total_processed_sources, 2);
        assert_eq!(stats.total_deleted_sources, 1);
        assert_eq!(stats.total_edges, 4);
        assert_eq!(stats.total_propagations, 5);
        assert_eq!(stats.total_llm_calls, 2);
        assert_eq!(stats.total_llm_tokens, 300);
        assert_eq!(stats.last_run_status.as_deref(), Some("no_op"));
        assert_eq!(stats.last_run_gate.as_deref(), Some("too_soon"));
        assert!(stats.last_success_at.is_some());
    }

    #[test]
    fn test_dream_stats_dry_run_is_not_success() {
        let mut dry = DreamRun::running(false, true);
        dry.status = DreamRunStatus::Success;
        dry.finish(DreamRunStatus::Success);

        let stats = DreamStats::from_runs(0, 0, &[dry]);

        assert_eq!(stats.total_runs, 1);
        assert_eq!(stats.dry_run_runs, 1);
        assert_eq!(stats.success_runs, 0);
        assert!(stats.last_success_at.is_none());
        assert!(stats.last_run_dry_run);
        assert_eq!(stats.last_run_status.as_deref(), Some("dry_run"));
    }
}

/// 场景结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneResult {
    pub scene: Scene,
}

/// 场景列表结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneListResult {
    pub scenes: Vec<Scene>,
    pub total: usize,
}
