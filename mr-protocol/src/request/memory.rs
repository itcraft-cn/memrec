//! 记忆检索相关的请求参数。

use serde::{Deserialize, Serialize};
use super::*;

/// 添加记忆的参数。
///
/// `working_dir` 用于服务端自动检测项目 ID（通过 `.mr_pid` 文件）。
/// `is_global` 为 true 时，该记忆对所有项目可见。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddParams {
    pub content: String,
    #[serde(default)]
    pub memory_type: MemoryType,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub is_global: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    /// 是否执行写入去重检测；None 时使用服务端配置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup: Option<bool>,
}

/// 获取单条记忆的参数。
///
/// `merge` 为 true 时，若该记忆属于分块组，则合并所有分块内容返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetParams {
    pub id: Uuid,
    #[serde(default)]
    pub merge: bool,
}

/// 更新记忆的参数。
///
/// 仅更新提供的字段，未提供的字段保持不变。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateParams {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// 删除记忆的参数。
///
/// `force` 为 true 时跳过软删除恢复期，直接永久删除。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteParams {
    pub id: Uuid,
    #[serde(default)]
    pub force: bool,
}

/// 精确搜索参数（按标签/时间范围/重要性过滤）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub mode: SearchMode,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<TimeRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default)]
    pub min_importance: f32,
}

/// 语义搜索参数，通过嵌入向量相似度检索记忆。
///
/// 搜索范围由 `include_global`/`project_only`/`global_only`/`cross_project` 四个标志控制，
/// 互斥关系由服务端处理。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryParams {
    /// 搜索查询文本，将被转换为嵌入向量
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    /// 是否包含公共记忆（默认 true）
    #[serde(default = "default_include_global")]
    pub include_global: bool,
    /// 仅搜索当前项目记忆
    #[serde(default)]
    pub project_only: bool,
    /// 仅搜索公共记忆
    #[serde(default)]
    pub global_only: bool,
    /// 跨所有项目搜索
    #[serde(default)]
    pub cross_project: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// 最低相似度阈值，低于此值的命中将被过滤
    #[serde(default = "default_min_score")]
    pub min_score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,
    #[serde(default = "default_mmr_enabled")]
    pub mmr_enabled: bool,
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieredSearchParams {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default = "default_include_global")]
    pub include_global: bool,
    #[serde(default)]
    pub project_only: bool,
    #[serde(default)]
    pub global_only: bool,
    #[serde(default)]
    pub cross_project: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_min_score")]
    pub min_score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(default = "default_max_total_tokens")]
    pub max_total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier_thresholds: Option<TierThresholdsParams>,
}

fn default_max_total_tokens() -> u32 {
    4000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierThresholdsParams {
    #[serde(default = "default_full_threshold")]
    pub full: f32,
    #[serde(default = "default_truncated_threshold")]
    pub truncated: f32,
    #[serde(default = "default_summary_threshold")]
    pub summary: f32,
}

fn default_full_threshold() -> f32 {
    0.8
}

fn default_truncated_threshold() -> f32 {
    0.5
}

fn default_summary_threshold() -> f32 {
    0.3
}

/// 搜索时是否包含公共记忆的默认值。
pub fn default_include_global() -> bool {
    true
}

pub fn default_hybrid_alpha() -> f64 {
    0.5
}

pub fn default_mmr_enabled() -> bool {
    true
}

pub fn default_mmr_lambda() -> f64 {
    0.5
}

/// 语义搜索最低相似度的默认值。
///
/// 优先读取 `MEMREC_MIN_SCORE` 环境变量，解析失败时回退到 0.5。
/// 注意：实际运行时由 `ModelType::default_min_score()` 覆盖（MiniLM=0.75, BGE-M3=0.5）。
pub fn default_min_score() -> f32 {
    std::env::var("MEMREC_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.5)
}

/// 搜索模式：精确匹配 / 语义检索 / 混合（默认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SearchMode {
    Exact,
    Semantic,
    #[default]
    Hybrid,
}

/// 时间范围过滤条件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// 搜索结果返回数量的默认值。
pub(super) fn default_top_k() -> usize {
    10
}

/// 列出记忆的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParams {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub project_only: bool,
    #[serde(default)]
    pub global_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    /// 是否包含已删除（软删除）的原始记录；默认 false（向后兼容）
    #[serde(default)]
    pub include_deleted: bool,
    /// 跳过前 N 条（与 limit 配合实现分页）；默认 0
    #[serde(default)]
    pub skip: usize,
}

/// 恢复软删除记忆的参数。
///
/// 将 `is_deleted=true` 的记忆恢复为活跃状态，并重新加入混合检索索引。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreParams {
    pub id: Uuid,
}

/// 列出记忆的默认数量限制。
pub(super) fn default_limit() -> usize {
    20
}

/// 为记忆添加标签的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagParams {
    pub id: Uuid,
    pub tag: String,
}

