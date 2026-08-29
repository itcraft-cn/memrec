//! # memrecd — MemRec 守护进程入口
//!
//! 启动守护进程，初始化日志、加载配置、创建存储和嵌入子系统，
//! 然后运行 Unix Socket 服务器等待客户端连接。
//!
//! ## 日志
//!
//! 日志行为由 `[log]` 配置控制：
//!
//! - `enabled`：是否写入文件（false 时仅输出 stdout）
//! - `level`：日志级别（trace/debug/info/warn/error）
//! - `rotation_days`：按天滚存并保留最近 N 个文件
//! - `memrecd_log`：日志文件完整路径，文件名部分作为滚存前缀
//!
//! ## 用法
//!
//! ```bash
//! memrecd              # 使用默认配置启动
//! memrecd --version    # 显示版本号
//! ```

use std::path::{Path, PathBuf};

use anyhow::Result;
use memrecd::config::DaemonConfig;
use tracing::{info, Level};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::FmtSubscriber;

use memrecd::daemon::Daemon;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("memrecd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config = DaemonConfig::load()?;

    let subscriber = build_subscriber(&config)?;
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting memrecd v{}", env!("CARGO_PKG_VERSION"));

    let daemon = Daemon::new()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("tokio-worker")
        .enable_all()
        .build()?;

    runtime.block_on(async { daemon.run().await })?;

    info!("memrecd stopped");
    Ok(())
}

/// 根据 `[log]` 配置构建日志订阅器。
///
/// - `enabled = true`：按天滚存写入文件，保留 `rotation_days` 个文件；
/// - `enabled = false`：仅输出 stdout。
///
/// `memrecd_log` 为完整路径，父目录作为日志目录，文件名拆分为滚存前缀/后缀。
fn build_subscriber(config: &DaemonConfig) -> Result<impl tracing::Subscriber + Send + Sync> {
    let level = parse_level(&config.log.level);

    // 统一 writer 为 BoxMakeWriter：文件滚存（默认）或 stdout（enabled=false）
    let writer = if config.log.enabled {
        let log_path = expand_tilde(&config.log.memrecd_log);
        let (log_dir, prefix, suffix) = split_log_path(&log_path);
        std::fs::create_dir_all(&log_dir)?;

        let file_appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(&prefix)
            .filename_suffix(&suffix)
            .max_log_files(config.log.rotation_days as usize)
            .build(&log_dir)?;
        BoxMakeWriter::new(file_appender)
    } else {
        BoxMakeWriter::new(std::io::stdout)
    };

    Ok(FmtSubscriber::builder()
        .with_max_level(level)
        .with_ansi(false)
        .with_writer(writer)
        .with_span_events(FmtSpan::CLOSE)
        .finish())
}

/// 解析日志级别字符串，未知值回退为 info。
fn parse_level(level: &str) -> Level {
    match level.to_ascii_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "warn" | "warning" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    }
}

/// 展开路径中的 `~` 前缀为家目录。
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest.strip_prefix('/').unwrap_or(rest));
        }
    }
    PathBuf::from(path)
}

/// 将日志文件完整路径拆分为（目录, 前缀, 后缀）。
///
/// 例如 `~/.memrec/logs/memrecd.log` → (`~/.memrec/logs`, `memrecd`, `log`)，
/// 供 RollingFileAppender 生成 `memrecd.YYYY-MM-DD.log` 形式的滚存文件。
fn split_log_path(path: &Path) -> (PathBuf, String, String) {
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("memrecd.log")
        .to_string();
    let (prefix, suffix) = match file_name.rsplit_once('.') {
        Some((p, s)) => (p.to_string(), s.to_string()),
        None => (file_name, "log".to_string()),
    };
    (dir, prefix, suffix)
}
