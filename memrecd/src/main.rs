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

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use memrecd::config::DaemonConfig;
use tracing::{info, Level};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::time::FormatTime;
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

/// 按本地时区（如 UTC+8）按天滚存的日志 writer。
///
/// `tracing-appender` 的 [`RollingFileAppender`] 按 UTC 切分日期（UTC 0 点切文件），
/// 与本地时区相差数小时，导致本地 0 点不切换日志文件。此 writer 使用
/// `chrono::Local` 计算日期，文件名形如 `memrecd.YYYY-MM-DD.log`，
/// 并按 `max_files` 清理过期文件（保留最近 N 个）。
#[derive(Clone)]
struct LocalDailyWriter {
    dir: PathBuf,
    prefix: String,
    suffix: String,
    max_files: usize,
    file: Arc<Mutex<Option<(String, File)>>>,
}

impl LocalDailyWriter {
    fn new(dir: PathBuf, prefix: String, suffix: String, max_files: usize) -> Self {
        Self {
            dir,
            prefix,
            suffix,
            max_files,
            file: Arc::new(Mutex::new(None)),
        }
    }

    /// 当前本地日期对应的滚存文件名。
    fn current_filename(&self) -> String {
        format!(
            "{}.{}.{}",
            self.prefix,
            chrono::Local::now().format("%Y-%m-%d"),
            self.suffix
        )
    }

    /// 清理过期滚存文件：匹配 `prefix.*.suffix` 模式，仅保留最近 `max_files` 个。
    fn cleanup_old_files(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let pattern_prefix = format!("{}.", self.prefix);
        let pattern_suffix = format!(".{}", self.suffix);

        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&pattern_prefix) && n.ends_with(&pattern_suffix))
                    .unwrap_or(false)
            })
            .collect();

        if files.len() <= self.max_files {
            return;
        }

        // 按文件名排序（YYYY-MM-DD 字典序 = 日期序），仅保留日期最新的 max_files 个。
        // 不用 mtime：批量写入时 mtime 可能相同，read_dir 顺序不保证，会误删当天文件。
        files.sort();
        let remove_count = files.len() - self.max_files;
        for old in files.into_iter().take(remove_count) {
            if let Err(e) = std::fs::remove_file(&old) {
                eprintln!("Failed to remove old log file {}: {}", old.display(), e);
            }
        }
    }
}

impl Write for LocalDailyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let filename = self.current_filename();
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());

        match &mut *guard {
            // 当天文件已打开：直接追加
            Some((cur, f)) if *cur == filename => f.write(buf),
            // 跨天（或首次）：切换文件句柄并清理过期文件
            _ => {
                let path = self.dir.join(&filename);
                let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
                let n = f.write(buf)?;
                *guard = Some((filename, f));
                self.cleanup_old_files();
                Ok(n)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some((_, f)) = &mut *self.file.lock().unwrap_or_else(|e| e.into_inner()) {
            f.flush()?;
        }
        Ok(())
    }
}

/// 本地时区时间戳格式器。
///
/// `FmtSubscriber` 默认输出 UTC 时间戳，这里改为本地时间（含时区偏移），
/// 便于按本地习惯阅读日志。
#[derive(Clone, Copy)]
struct LocalTimestamp;

impl FormatTime for LocalTimestamp {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(
            w,
            "{}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z")
        )
    }
}

/// 根据 `[log]` 配置构建日志订阅器。
///
/// - `enabled = true`：按**本地时区**按天滚存写入文件，保留 `rotation_days` 个文件；
/// - `enabled = false`：仅输出 stdout。
///
/// `memrecd_log` 为完整路径，父目录作为日志目录，文件名拆分为滚存前缀/后缀。
fn build_subscriber(config: &DaemonConfig) -> Result<impl tracing::Subscriber + Send + Sync> {
    let level = parse_level(&config.log.level);

    // 统一 writer 为 BoxMakeWriter：本地时区文件滚存（默认）或 stdout（enabled=false）
    let writer = if config.log.enabled {
        let log_path = expand_tilde(&config.log.memrecd_log);
        let (log_dir, prefix, suffix) = split_log_path(&log_path);
        std::fs::create_dir_all(&log_dir)?;

        let file_writer =
            LocalDailyWriter::new(log_dir, prefix, suffix, config.log.rotation_days as usize);
        // 闭包 MakeWriter：每次写入返回共享句柄的克隆（跨线程安全）
        BoxMakeWriter::new(move || file_writer.clone())
    } else {
        BoxMakeWriter::new(std::io::stdout)
    };

    Ok(FmtSubscriber::builder()
        .with_max_level(level)
        .with_ansi(false)
        .with_writer(writer)
        .with_timer(LocalTimestamp)
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
/// 供滚存 writer 生成 `memrecd.YYYY-MM-DD.log` 形式的滚存文件。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_daily_writer_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = LocalDailyWriter::new(
            dir.path().to_path_buf(),
            "memrecd".to_string(),
            "log".to_string(),
            7,
        );

        writer.write_all(b"hello local time log\n").expect("write");
        writer.flush().expect("flush");

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let expected = dir.path().join(format!("memrecd.{}.log", today));
        assert!(expected.exists(), "expected {}", expected.display());
        let content = std::fs::read_to_string(&expected).unwrap();
        assert!(content.contains("hello local time log"));
    }

    #[test]
    fn test_local_daily_writer_switches_date() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = LocalDailyWriter::new(
            dir.path().to_path_buf(),
            "memrecd".to_string(),
            "log".to_string(),
            7,
        );

        writer.write_all(b"line1\n").unwrap();
        // 模拟跨天：预置一个"昨天"的文件，触发切换后清理逻辑应保留它（未超保留数）
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let yesterday_path = dir.path().join(format!("memrecd.{}.log", yesterday));
        std::fs::write(&yesterday_path, "old\n").unwrap();

        writer.write_all(b"line2\n").unwrap();
        writer.flush().unwrap();

        assert!(yesterday_path.exists(), "yesterday log kept");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert!(dir.path().join(format!("memrecd.{}.log", today)).exists());
    }

    #[test]
    fn test_local_daily_writer_cleanup_excess_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = LocalDailyWriter::new(
            dir.path().to_path_buf(),
            "memrecd".to_string(),
            "log".to_string(),
            2,
        );

        // 预置 4 个历史滚存文件（修改时间递旧），max_files=2
        for i in 0..4 {
            let day = (chrono::Local::now() - chrono::Duration::days(i as i64 + 1))
                .format("%Y-%m-%d")
                .to_string();
            let p = dir.path().join(format!("memrecd.{}.log", day));
            std::fs::write(&p, format!("old {}\n", i)).unwrap();
            // 设定修改时间：越旧的 i 越小越旧？这里用真实时间即可（写入顺序决定 mtime）
        }

        // 写入触发 cleanup
        writer.write_all(b"new\n").unwrap();
        writer.flush().unwrap();

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut remaining: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("memrecd."))
            .collect();
        remaining.sort();
        // 今天 + 最近 1 个历史 = 2 个
        assert!(remaining.iter().any(|n| n.contains(&today)));
        assert!(remaining.len() <= 2, "remaining: {:?}", remaining);
    }
}
