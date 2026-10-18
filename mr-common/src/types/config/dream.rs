//! Dream 整合配置（含各字段默认值与 fuse 重构新增项）。

use serde::{Deserialize, Serialize};

/// Dream 后台整合配置。
///
/// 控制记忆压缩和整合行为，类似人类睡眠中的记忆巩固过程。
/// 支持4阶段处理：交叉项目提取、个人提炼、清理、向量重生成。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamConfig {
    /// 是否启用 Dream 整合
    #[serde(default = "default_dream_enabled")]
    pub enabled: bool,

    /// 是否要求配置 LLM 才能执行。
    /// 为 true 且未接入 LLM 时，Dream 视为不启用（Gate 检查失败）
    #[serde(default = "default_requires_llm")]
    pub requires_llm: bool,

    /// 触发整合的最小记忆数量
    #[serde(default = "default_min_memories")]
    pub min_memories: usize,

    /// 整合的记忆最大年龄（小时），默认 168h = 7天
    #[serde(default = "default_max_age_hours")]
    pub max_age_hours: u64,

    /// 两次整合之间的最小间隔（小时）
    #[serde(default = "default_min_hours_between")]
    pub min_hours_between: f64,

    /// 两次整合之间需新增的最小记忆数（新记忆门槛）。
    /// 距上次执行后新增记忆不足此值时，Dream 不执行，避免无新数据时空转。
    #[serde(default = "default_min_new_memories")]
    pub min_new_memories: usize,

    /// 整合后生成的记忆类型
    #[serde(default = "default_integration_type")]
    pub integration_type: String,

    /// 整合后生成的记忆标签
    #[serde(default = "default_integration_tags")]
    pub integration_tags: Vec<String>,

    /// Phase 1: 交叉项目记忆提取开关
    #[serde(default = "default_phase_cross_project")]
    pub phase_cross_project: bool,

    /// Phase 2: 个人总体记忆提炼开关
    #[serde(default = "default_phase_personal_summary")]
    pub phase_personal_summary: bool,

    /// Phase 3: 记忆清理开关
    #[serde(default = "default_phase_cleanup")]
    pub phase_cleanup: bool,

    /// Phase 4: 向量重新生成开关（资源消耗大，默认关闭）
    #[serde(default = "default_phase_vector_regen")]
    pub phase_vector_regen: bool,

    /// 单次整合的最大记忆条数（防止 LLM 输入超窗）
    #[serde(default = "default_integration_max_memories")]
    pub integration_max_memories: usize,

    /// 高价值记忆保留阈值：importance 不低于此值的记忆不参与整合与删除
    #[serde(default = "default_integration_preserve_importance")]
    pub integration_preserve_importance: f32,

    /// 高价值记忆保留标签：命中任一标签的记忆不参与整合与删除
    #[serde(default = "default_integration_preserve_tags")]
    pub integration_preserve_tags: Vec<String>,

    /// 保留的记忆类型：这些类型的记忆不参与整合与删除（保留原文）。
    /// 如 `context`（配置/环境信息，压缩后细节不可恢复）与 `preference`（用户偏好）。
    #[serde(default = "default_integration_preserve_types")]
    pub integration_preserve_types: Vec<String>,

    /// 参与整合的记忆最小内容长度（字符），过短的测试/占位记录不整合
    #[serde(default = "default_integration_min_content_len")]
    pub integration_min_content_len: usize,

    /// Phase 3 清理：不活跃天数门槛
    #[serde(default = "default_cleanup_inactive_days")]
    pub cleanup_inactive_days: u64,

    /// Phase 3 清理：重要性下限，低于此值的旧记忆被删除
    #[serde(default = "default_cleanup_importance_threshold")]
    pub cleanup_importance_threshold: f32,

    /// 批处理大小
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// 批处理间隔（毫秒）
    #[serde(default = "default_batch_interval_ms")]
    pub batch_interval_ms: u64,

    // ---- Stage F 融合（fusion）----
    /// 是否启用语义聚类
    #[serde(default = "default_fusion_cluster_enabled")]
    pub fusion_cluster_enabled: bool,
    /// 聚类相似度阈值
    #[serde(default = "default_fusion_cluster_threshold")]
    pub fusion_cluster_threshold: f32,
    /// 单簇上限（超出按相似度拆分为多个子簇）
    #[serde(default = "default_fusion_cluster_max_size")]
    pub fusion_cluster_max_size: usize,
    /// 单次最大簇数（超出合并最小的若干簇）
    #[serde(default = "default_fusion_max_clusters")]
    pub fusion_max_clusters: usize,
    /// 是否产出事实条
    #[serde(default = "default_fusion_emit_facts")]
    pub fusion_emit_facts: bool,
    /// 是否产出主题摘要
    #[serde(default = "default_fusion_emit_summary")]
    pub fusion_emit_summary: bool,
    /// 是否做矛盾消解（识别矛盾对，转为 Contradicts 边）
    #[serde(default = "default_fusion_detect_contradictions")]
    pub fusion_detect_contradictions: bool,
    /// 是否对融合产出的事实条做近重复去重（上游去重，默认开）
    #[serde(default = "default_fusion_dedup_enabled")]
    pub fusion_dedup_enabled: bool,
    /// 事实条近重复判定阈值（余弦相似度，默认 0.97）；达到即视为重复，
    /// 仅保留最先产出的一条，避免 Dream 产出冗余记忆与噪声边。
    #[serde(default = "default_fusion_dedup_threshold")]
    pub fusion_dedup_threshold: f32,

    // ---- Stage N 成网（networking）----
    /// 是否启用成网段
    #[serde(default = "default_networking_enabled")]
    pub networking_enabled: bool,
    /// Similar 边近邻数
    #[serde(default = "default_networking_similar_topk")]
    pub networking_similar_topk: usize,
    /// Similar 边阈值
    #[serde(default = "default_networking_similar_threshold")]
    pub networking_similar_threshold: f32,
    /// 是否启用 LLM 语义边（默认关，先零 LLM 跑图）
    #[serde(default = "default_networking_semantic_enabled")]
    pub networking_semantic_enabled: bool,
    /// 语义边候选池上限：按向量相似度取最相近的若干条送入 LLM（默认 20），
    /// 避免把全量候选+产出整体入参导致成本随规模线性膨胀。
    #[serde(default = "default_networking_semantic_max_candidates")]
    pub networking_semantic_max_candidates: usize,
    /// 语义边候选的近重复排除阈值（余弦，默认 0.97）：达到即视为重复，
    /// 不参与语义边生成（重复应去重，而非建立关系边）。
    #[serde(default = "default_networking_semantic_dup_threshold")]
    pub networking_semantic_dup_threshold: f32,
    /// 是否启用传播
    #[serde(default = "default_networking_propagation_enabled")]
    pub networking_propagation_enabled: bool,

    // ---- 检索图扩散（retrieval）----
    /// 检索是否走图扩散
    #[serde(default = "default_retrieval_graph_enabled")]
    pub retrieval_graph_enabled: bool,
    /// 扩散跳数
    #[serde(default = "default_retrieval_graph_depth")]
    pub retrieval_graph_depth: usize,
    /// 扩散衰减
    #[serde(default = "default_retrieval_graph_decay")]
    pub retrieval_graph_decay: f32,
    /// 图命中并入候选的权重
    #[serde(default = "default_retrieval_graph_weight")]
    pub retrieval_graph_weight: f32,
    /// 图命中保留名额比例（0.0-1.0，默认 0.25）：最终 top_k 中为图命中专留
    /// 的比例，使图命中不被种子分挤压后截断。置 0 则退回"纯按分合并截断"。
    #[serde(default = "default_retrieval_graph_quota")]
    pub retrieval_graph_quota: f32,

    // ---- dry-run ----
    /// dry-run 预演时是否真调 LLM（关闭时用模板占位摘要）
    #[serde(default = "default_dry_run_with_llm")]
    pub dry_run_with_llm: bool,
}

fn default_dream_enabled() -> bool {
    false
}
fn default_requires_llm() -> bool {
    true
}
fn default_min_memories() -> usize {
    20
}
fn default_max_age_hours() -> u64 {
    168
}
fn default_min_hours_between() -> f64 {
    24.0
}

fn default_min_new_memories() -> usize {
    1
}
fn default_integration_type() -> String {
    "knowledge".to_string()
}
fn default_integration_tags() -> Vec<String> {
    vec!["dream-integrated".to_string()]
}
fn default_phase_cross_project() -> bool {
    true
}
fn default_phase_personal_summary() -> bool {
    true
}
fn default_phase_cleanup() -> bool {
    true
}
fn default_phase_vector_regen() -> bool {
    false
}
fn default_integration_max_memories() -> usize {
    100
}
fn default_integration_preserve_importance() -> f32 {
    0.85
}
fn default_integration_preserve_tags() -> Vec<String> {
    vec!["critical".to_string()]
}
fn default_integration_preserve_types() -> Vec<String> {
    vec!["preference".to_string(), "context".to_string()]
}
fn default_integration_min_content_len() -> usize {
    10
}
fn default_cleanup_inactive_days() -> u64 {
    90
}
fn default_cleanup_importance_threshold() -> f32 {
    0.1
}
fn default_batch_size() -> usize {
    100
}
fn default_batch_interval_ms() -> u64 {
    1000
}

fn default_fusion_cluster_enabled() -> bool {
    true
}
fn default_fusion_cluster_threshold() -> f32 {
    0.78
}
fn default_fusion_cluster_max_size() -> usize {
    20
}
fn default_fusion_max_clusters() -> usize {
    12
}
fn default_fusion_emit_facts() -> bool {
    true
}
fn default_fusion_emit_summary() -> bool {
    true
}
fn default_fusion_detect_contradictions() -> bool {
    true
}
fn default_fusion_dedup_enabled() -> bool {
    true
}
fn default_fusion_dedup_threshold() -> f32 {
    0.97
}
fn default_networking_enabled() -> bool {
    true
}
fn default_networking_similar_topk() -> usize {
    5
}
fn default_networking_similar_threshold() -> f32 {
    0.82
}
fn default_networking_semantic_enabled() -> bool {
    false
}
fn default_networking_semantic_max_candidates() -> usize {
    20
}
fn default_networking_semantic_dup_threshold() -> f32 {
    0.97
}
fn default_networking_propagation_enabled() -> bool {
    true
}
fn default_retrieval_graph_enabled() -> bool {
    true
}
fn default_retrieval_graph_depth() -> usize {
    1
}
fn default_retrieval_graph_decay() -> f32 {
    0.6
}
fn default_retrieval_graph_weight() -> f32 {
    0.3
}
fn default_retrieval_graph_quota() -> f32 {
    0.25
}
fn default_dry_run_with_llm() -> bool {
    false
}

impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            requires_llm: true,
            min_memories: 20,
            max_age_hours: 168,
            min_hours_between: 24.0,
            min_new_memories: 1,
            integration_type: "knowledge".to_string(),
            integration_tags: vec!["dream-integrated".to_string()],
            phase_cross_project: true,
            phase_personal_summary: true,
            phase_cleanup: true,
            phase_vector_regen: false,
            integration_max_memories: 100,
            integration_preserve_importance: 0.85,
            integration_preserve_tags: vec!["critical".to_string()],
            integration_preserve_types: vec!["preference".to_string(), "context".to_string()],
            integration_min_content_len: 10,
            cleanup_inactive_days: 90,
            cleanup_importance_threshold: 0.1,
            batch_size: 100,
            batch_interval_ms: 1000,
            fusion_cluster_enabled: true,
            fusion_cluster_threshold: 0.78,
            fusion_cluster_max_size: 20,
            fusion_max_clusters: 12,
            fusion_emit_facts: true,
            fusion_emit_summary: true,
            fusion_detect_contradictions: true,
            fusion_dedup_enabled: true,
            fusion_dedup_threshold: 0.97,
            networking_enabled: true,
            networking_similar_topk: 5,
            networking_similar_threshold: 0.82,
            networking_semantic_enabled: false,
            networking_semantic_max_candidates: 20,
            networking_semantic_dup_threshold: 0.97,
            networking_propagation_enabled: true,
            retrieval_graph_enabled: true,
            retrieval_graph_depth: 1,
            retrieval_graph_decay: 0.6,
            retrieval_graph_weight: 0.3,
            retrieval_graph_quota: 0.25,
            dry_run_with_llm: false,
        }
    }
}
