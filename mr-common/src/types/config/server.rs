//! 服务端与日志配置。

use serde::{Deserialize, Serialize};

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

