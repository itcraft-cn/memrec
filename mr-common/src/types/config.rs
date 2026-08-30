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

use serde::{Deserialize, Serialize};

/// 记忆生命周期管理配置。
///
/// 控制记忆的软删除恢复、硬删除淘汰、存储压缩等策略。
/// 水位线机制：当存储使用率达到 `high_watermark` 时触发淘汰，降至 `low_watermark` 时停止。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// 软删除后的恢复天数，超过此期限将无法恢复
    pub soft_delete_recovery_days: u32,
    /// 重要性低于此阈值的记忆可被硬删除淘汰
    pub hard_delete_importance: f32,
    /// 超过此天数未访问的低重要性记忆可被淘汰
    pub hard_delete_inactive_days: u32,
    /// 重要性低于此阈值的记忆可被压缩
    pub compression_importance: f32,
    /// 最大存储容量（GB）
    pub max_storage_gb: usize,
    /// 高水位线，存储使用率达到此值时触发淘汰
    pub high_watermark: f32,
    /// 低水位线，淘汰至此值时停止
    pub low_watermark: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            soft_delete_recovery_days: 30,
            hard_delete_importance: 0.1,
            hard_delete_inactive_days: 90,
            compression_importance: 0.3,
            max_storage_gb: 10,
            high_watermark: 0.9,
            low_watermark: 0.7,
        }
    }
}

/// 重要性计算权重配置。
///
/// 重要性评分公式为各维度加权和：
/// `score = weight_recency * recency + weight_frequency * frequency + weight_semantic * semantic + weight_explicit * explicit`
///
/// `lambda` 控制时效性衰减速率，`frequency_normalize` 控制频率归一化基数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportanceConfig {
    /// 时效性衰减系数，值越大衰减越快
    pub lambda: f32,
    /// 频率归一化基数，访问次数除以此值得到归一化频率
    pub frequency_normalize: f32,
    /// 时效性维度权重
    pub weight_recency: f32,
    /// 访问频率维度权重
    pub weight_frequency: f32,
    /// 语义相关性维度权重
    pub weight_semantic: f32,
    /// 显式评分维度权重（用户通过 tag 重要性标记）
    pub weight_explicit: f32,
}

impl Default for ImportanceConfig {
    fn default() -> Self {
        Self {
            lambda: 0.05,
            frequency_normalize: 10.0,
            weight_recency: 0.3,
            weight_frequency: 0.2,
            weight_semantic: 0.2,
            weight_explicit: 0.3,
        }
    }
}

/// 记忆来源权重配置。
///
/// 不同来源的记忆可信度不同，在搜索评分中体现。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceWeights {
    #[serde(default = "default_user_weight")]
    pub user: f64,
    #[serde(default = "default_system_weight")]
    pub system: f64,
    #[serde(default = "default_inferred_weight")]
    pub inferred: f64,
    #[serde(default = "default_external_weight")]
    pub external: f64,
}

fn default_user_weight() -> f64 {
    1.0
}
fn default_system_weight() -> f64 {
    0.8
}
fn default_inferred_weight() -> f64 {
    0.5
}
fn default_external_weight() -> f64 {
    0.7
}

impl Default for SourceWeights {
    fn default() -> Self {
        Self {
            user: 1.0,
            system: 0.8,
            inferred: 0.5,
            external: 0.7,
        }
    }
}

/// 搜索配置。
///
/// 控制混合检索、MMR 重排、时间衰减等行为。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// 向量检索权重，默认 0.5（BM25 权重为 1 - alpha）
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,

    /// 是否启用 MMR 重排
    #[serde(default = "default_mmr_enabled")]
    pub mmr_enabled: bool,

    /// MMR 相关性权重，默认 0.5
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,

    /// 时间衰减半衰期（小时），默认 336（14天）
    #[serde(default = "default_decay_half_life")]
    pub decay_half_life_hours: f64,

    /// 来源权重
    #[serde(default)]
    pub source_weights: SourceWeights,

    /// 豁免时间衰减的作用域
    #[serde(default = "default_evergreen_scopes")]
    pub evergreen_scopes: Vec<String>,
}

fn default_hybrid_alpha() -> f64 {
    0.5
}
fn default_mmr_enabled() -> bool {
    true
}
fn default_mmr_lambda() -> f64 {
    0.5
}
fn default_decay_half_life() -> f64 {
    336.0
}
fn default_evergreen_scopes() -> Vec<String> {
    vec!["global".to_string(), "workspace".to_string()]
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            hybrid_alpha: 0.5,
            mmr_enabled: true,
            mmr_lambda: 0.5,
            decay_half_life_hours: 336.0,
            source_weights: SourceWeights::default(),
            evergreen_scopes: vec!["global".to_string(), "workspace".to_string()],
        }
    }
}

/// 服务端路径配置。
///
/// 路径中可包含 `~` 前缀，由守护进程在加载时通过 `shellexpand` 展开。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Unix Socket 监听路径
    pub socket_path: String,
    /// RocksDB 数据存储目录
    pub data_dir: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            socket_path: "~/.memrec/memrecd.sock".to_string(),
            data_dir: "~/.memrec/data".to_string(),
        }
    }
}

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
        }
    }
}

/// 系统空闲监控配置。
///
/// 控制系统空闲检测行为，用于自动触发 Dream 整合。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleMonitorConfig {
    /// 是否启用空闲监控
    #[serde(default = "default_idle_enabled")]
    pub enabled: bool,

    /// 检查间隔（秒），默认 300 = 5分钟
    #[serde(default = "default_check_interval_secs")]
    pub check_interval_secs: u64,

    /// load average 阈值，低于此值视为空闲
    #[serde(default = "default_load_threshold")]
    pub load_threshold: f64,

    /// 是否启用自动触发 dream
    #[serde(default = "default_auto_dream_enabled")]
    pub auto_dream_enabled: bool,
}

fn default_idle_enabled() -> bool {
    true
}
fn default_check_interval_secs() -> u64 {
    300
}
fn default_load_threshold() -> f64 {
    0.5
}
fn default_auto_dream_enabled() -> bool {
    true
}

impl Default for IdleMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 300,
            load_threshold: 0.5,
            auto_dream_enabled: true,
        }
    }
}

/// 队列配置。
///
/// 控制请求队列容量和行为。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    /// Normal Queue 容量
    #[serde(default = "default_normal_queue_capacity")]
    pub normal_queue_capacity: usize,

    /// Dream Queue 容量
    #[serde(default = "default_dream_queue_capacity")]
    pub dream_queue_capacity: usize,
}

fn default_normal_queue_capacity() -> usize {
    1000
}
fn default_dream_queue_capacity() -> usize {
    10
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            normal_queue_capacity: 1000,
            dream_queue_capacity: 10,
        }
    }
}

/// 日志配置。
///
/// 控制日志文件位置、滚存和清理策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// 是否启用文件日志；为 false 时仅输出到 stdout
    #[serde(default = "default_log_enabled")]
    pub enabled: bool,

    /// 日志级别（trace/debug/info/warn/error）
    #[serde(default = "default_log_level")]
    pub level: String,

    /// 日志保留天数（按天滚存，保留最近 N 个文件）
    #[serde(default = "default_rotation_days")]
    pub rotation_days: u64,

    /// memrecd 主日志文件路径
    #[serde(default = "default_memrecd_log_path")]
    pub memrecd_log: String,
}

fn default_log_enabled() -> bool {
    true
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_rotation_days() -> u64 {
    7
}
fn default_memrecd_log_path() -> String {
    "~/.memrec/logs/memrecd.log".to_string()
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: "info".to_string(),
            rotation_days: 7,
            memrecd_log: "~/.memrec/logs/memrecd.log".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_config_defaults() {
        let config = MemoryConfig::default();

        assert_eq!(config.soft_delete_recovery_days, 30);
        assert_eq!(config.hard_delete_importance, 0.1);
        assert_eq!(config.max_storage_gb, 10);
        assert_eq!(config.high_watermark, 0.9);
    }

    #[test]
    fn test_importance_config_defaults() {
        let config = ImportanceConfig::default();

        assert_eq!(config.lambda, 0.05);
        assert_eq!(config.weight_recency, 0.3);
    }

    #[test]
    fn test_config_serde() {
        let config = MemoryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: MemoryConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            config.soft_delete_recovery_days,
            parsed.soft_delete_recovery_days
        );
    }

    #[test]
    fn test_source_weights_defaults() {
        let weights = SourceWeights::default();

        assert_eq!(weights.user, 1.0);
        assert_eq!(weights.system, 0.8);
        assert_eq!(weights.inferred, 0.5);
        assert_eq!(weights.external, 0.7);
    }

    #[test]
    fn test_source_weights_serde_roundtrip() {
        let weights = SourceWeights::default();
        let json = serde_json::to_string(&weights).unwrap();
        let parsed: SourceWeights = serde_json::from_str(&json).unwrap();

        assert_eq!(weights.user, parsed.user);
        assert_eq!(weights.system, parsed.system);
        assert_eq!(weights.inferred, parsed.inferred);
        assert_eq!(weights.external, parsed.external);
    }

    #[test]
    fn test_source_weights_partial_deserialize() {
        let json = r#"{"user": 2.0}"#;
        let parsed: SourceWeights = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.user, 2.0);
        assert_eq!(parsed.system, 0.8);
        assert_eq!(parsed.inferred, 0.5);
        assert_eq!(parsed.external, 0.7);
    }

    #[test]
    fn test_search_config_defaults() {
        let config = SearchConfig::default();

        assert_eq!(config.hybrid_alpha, 0.5);
        assert!(config.mmr_enabled);
        assert_eq!(config.mmr_lambda, 0.5);
        assert_eq!(config.decay_half_life_hours, 336.0);
        assert_eq!(config.source_weights.user, 1.0);
        assert_eq!(config.evergreen_scopes, vec!["global", "workspace"]);
    }

    #[test]
    fn test_search_config_serde_roundtrip() {
        let config = SearchConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: SearchConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.hybrid_alpha, parsed.hybrid_alpha);
        assert_eq!(config.mmr_enabled, parsed.mmr_enabled);
        assert_eq!(config.mmr_lambda, parsed.mmr_lambda);
        assert_eq!(config.decay_half_life_hours, parsed.decay_half_life_hours);
        assert_eq!(config.source_weights.user, parsed.source_weights.user);
        assert_eq!(config.evergreen_scopes, parsed.evergreen_scopes);
    }

    #[test]
    fn test_search_config_partial_deserialize() {
        let json = r#"{"hybrid_alpha": 0.8, "mmr_enabled": false}"#;
        let parsed: SearchConfig = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.hybrid_alpha, 0.8);
        assert!(!parsed.mmr_enabled);
        assert_eq!(parsed.mmr_lambda, 0.5);
        assert_eq!(parsed.decay_half_life_hours, 336.0);
        assert_eq!(parsed.source_weights.user, 1.0);
        assert_eq!(parsed.evergreen_scopes, vec!["global", "workspace"]);
    }

    #[test]
    fn test_dream_config_defaults() {
        let config = DreamConfig::default();

        assert!(!config.enabled);
        assert!(config.requires_llm);
        assert_eq!(config.min_memories, 20);
        assert_eq!(config.max_age_hours, 168);
        assert_eq!(config.min_hours_between, 24.0);
        assert_eq!(config.integration_type, "knowledge");
        assert_eq!(config.integration_tags, vec!["dream-integrated"]);
    }

    #[test]
    fn test_dream_config_integration_defaults() {
        let config = DreamConfig::default();

        assert_eq!(config.integration_max_memories, 100);
        assert_eq!(config.integration_preserve_importance, 0.85);
        assert_eq!(
            config.integration_preserve_tags,
            vec!["critical".to_string()]
        );
        assert_eq!(
            config.integration_preserve_types,
            vec!["preference".to_string(), "context".to_string()]
        );
        assert_eq!(config.integration_min_content_len, 10);
        assert_eq!(config.cleanup_inactive_days, 90);
        assert_eq!(config.cleanup_importance_threshold, 0.1);
    }

    #[test]
    fn test_dream_config_integration_serde() {
        let json = r#"{"enabled": true, "integration_max_memories": 50, "integration_preserve_importance": 0.9, "cleanup_inactive_days": 30}"#;
        let parsed: DreamConfig = serde_json::from_str(json).unwrap();

        assert!(parsed.enabled);
        assert_eq!(parsed.integration_max_memories, 50);
        assert_eq!(parsed.integration_preserve_importance, 0.9);
        assert_eq!(parsed.cleanup_inactive_days, 30);
        // 未指定的新字段走默认值
        assert_eq!(
            parsed.integration_preserve_tags,
            vec!["critical".to_string()]
        );
        assert_eq!(parsed.cleanup_importance_threshold, 0.1);
    }

    #[test]
    fn test_dream_config_serde_roundtrip() {
        let config = DreamConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: DreamConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.enabled, parsed.enabled);
        assert_eq!(config.min_memories, parsed.min_memories);
        assert_eq!(config.max_age_hours, parsed.max_age_hours);
        assert_eq!(config.min_hours_between, parsed.min_hours_between);
    }

    #[test]
    fn test_dream_config_partial_deserialize() {
        let json = r#"{"enabled": true, "min_memories": 50}"#;
        let parsed: DreamConfig = serde_json::from_str(json).unwrap();

        assert!(parsed.enabled);
        assert!(parsed.requires_llm);
        assert_eq!(parsed.min_memories, 50);
        assert_eq!(parsed.max_age_hours, 168);
        assert_eq!(parsed.min_hours_between, 24.0);
    }

    #[test]
    fn test_dream_config_new_fields() {
        let config = DreamConfig::default();

        assert!(config.phase_cross_project);
        assert!(config.phase_personal_summary);
        assert!(config.phase_cleanup);
        assert!(!config.phase_vector_regen);
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.batch_interval_ms, 1000);
    }

    #[test]
    fn test_dream_config_serde_new_fields() {
        let json = r#"{"enabled": true, "phase_cross_project": false, "batch_size": 50}"#;
        let parsed: DreamConfig = serde_json::from_str(json).unwrap();

        assert!(parsed.enabled);
        assert!(parsed.requires_llm);
        assert!(!parsed.phase_cross_project);
        assert!(parsed.phase_personal_summary);
        assert_eq!(parsed.batch_size, 50);
        assert_eq!(parsed.batch_interval_ms, 1000);
    }

    #[test]
    fn test_idle_monitor_config_defaults() {
        let config = IdleMonitorConfig::default();

        assert!(config.enabled);
        assert_eq!(config.check_interval_secs, 300);
        assert_eq!(config.load_threshold, 0.5);
        assert!(config.auto_dream_enabled);
    }

    #[test]
    fn test_idle_monitor_config_serde() {
        let config = IdleMonitorConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: IdleMonitorConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.check_interval_secs, parsed.check_interval_secs);
    }

    #[test]
    fn test_idle_monitor_config_partial_deserialize() {
        let json = r#"{"enabled": false, "load_threshold": 1.0}"#;
        let parsed: IdleMonitorConfig = serde_json::from_str(json).unwrap();

        assert!(!parsed.enabled);
        assert_eq!(parsed.load_threshold, 1.0);
        assert_eq!(parsed.check_interval_secs, 300);
    }

    #[test]
    fn test_queue_config_defaults() {
        let config = QueueConfig::default();

        assert_eq!(config.normal_queue_capacity, 1000);
        assert_eq!(config.dream_queue_capacity, 10);
    }

    #[test]
    fn test_queue_config_serde() {
        let json = r#"{"normal_queue_capacity": 500}"#;
        let parsed: QueueConfig = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.normal_queue_capacity, 500);
        assert_eq!(parsed.dream_queue_capacity, 10);
    }

    #[test]
    fn test_log_config_defaults() {
        let config = LogConfig::default();

        assert!(config.enabled);
        assert_eq!(config.level, "info");
        assert_eq!(config.rotation_days, 7);
        assert!(config.memrecd_log.contains("memrecd.log"));
    }

    #[test]
    fn test_log_config_serde() {
        let json = r#"{"rotation_days": 14, "level": "debug"}"#;
        let parsed: LogConfig = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.rotation_days, 14);
        assert_eq!(parsed.level, "debug");
        assert!(parsed.enabled);
    }
}
