//! 空闲监控与 Dream 队列配置。

use serde::{Deserialize, Serialize};

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

