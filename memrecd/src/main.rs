//! # memrecd — MemRec 守护进程入口
//!
//! 启动守护进程，初始化日志、加载配置、创建存储和嵌入子系统，
//! 然后运行 Unix Socket 服务器等待客户端连接。
//!
//! ## 用法
//!
//! ```bash
//! memrecd              # 使用默认配置启动
//! memrecd --version    # 显示版本号
//! ```

use anyhow::Result;
use memrecd::config::DaemonConfig;
use tracing::{info, Level};
use tracing_appender::rolling::RollingFileAppender;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::FmtSubscriber;

use memrecd::daemon::Daemon;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("memrecd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config = DaemonConfig::load()?;

    let home = dirs::home_dir().expect("Failed to get home directory");
    let log_path = if config.log.memrecd_log.starts_with('~') {
        let rest = config.log.memrecd_log.strip_prefix('~').unwrap_or(&config.log.memrecd_log);
        home.join(rest.strip_prefix('/').unwrap_or(rest))
    } else {
        std::path::PathBuf::from(&config.log.memrecd_log)
    };

    let log_dir = log_path.parent().unwrap();
    std::fs::create_dir_all(log_dir)?;

    let file_appender = RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::NEVER)
        .filename_prefix("memrecd")
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir)?;

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_ansi(false)
        .with_writer(file_appender)
        .with_span_events(FmtSpan::CLOSE)
        .finish();

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
