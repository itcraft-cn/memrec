//! # DreamLock 并发控制
//!
//! 防止多个 Dream 进程并发执行，导致重复整合。
//!
//! ## 锁文件
//!
//! - 路径：`~/.memrec/dream.lock`
//! - 内容：JSON 格式的 PID、时间戳、会话计数
//!
//! ## 僵锁检测
//!
//! 若锁文件存在但持有进程已退出（通过 `/proc/{pid}` 检测），则自动清理。

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use memrec_common::types::DreamConfig;

#[derive(Debug, Error)]
pub enum DreamLockError {
    #[error("Lock file exists and process {pid} is still alive")]
    AlreadyLocked { pid: u32 },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// 锁文件内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockContent {
    /// 持有锁的进程 PID
    pub pid: u32,
    /// 获取锁的时间戳（Unix timestamp, 秒）
    pub timestamp: i64,
    /// 会话计数（可选，用于 DreamGate 检查）
    pub session_count: u32,
}

/// DreamLock 并发控制。
pub struct DreamLock {
    lock_path: PathBuf,
}

impl DreamLock {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            lock_path: data_dir.join("dream.lock"),
        }
    }

    /// 尝试获取锁。
    ///
    /// 返回 `Ok(true)` 表示成功获取锁。
    /// 返回 `Ok(false)` 表示锁已被其他进程持有。
    /// 返回 `Err` 表示发生错误。
    pub fn try_acquire(&self, session_count: u32) -> Result<bool, DreamLockError> {
        if self.lock_path.exists() {
            let content = self.read_lock()?;

            if self.is_process_alive(content.pid) {
                return Ok(false);
            }

            self.cleanup_stale_lock()?;
        }

        self.write_lock(session_count)?;
        Ok(true)
    }

    /// 释放锁。
    pub fn release(&self) -> Result<(), DreamLockError> {
        if self.lock_path.exists() {
            fs::remove_file(&self.lock_path)?;
        }
        Ok(())
    }

    /// 读取锁文件内容。
    fn read_lock(&self) -> Result<LockContent, DreamLockError> {
        let mut file = fs::File::open(&self.lock_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        let lock: LockContent = serde_json::from_str(&content)?;
        Ok(lock)
    }

    /// 写入锁文件。
    fn write_lock(&self, session_count: u32) -> Result<(), DreamLockError> {
        let content = LockContent {
            pid: std::process::id(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            session_count,
        };

        let json = serde_json::to_string(&content)?;
        let mut file = fs::File::create(&self.lock_path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    /// 检查进程是否存活（Linux only）。
    fn is_process_alive(&self, pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }

    /// 清理僵锁。
    fn cleanup_stale_lock(&self) -> Result<(), DreamLockError> {
        fs::remove_file(&self.lock_path)?;
        Ok(())
    }

    /// 获取当前锁内容（如果存在）。
    pub fn get_lock_content(&self) -> Option<LockContent> {
        if self.lock_path.exists() {
            self.read_lock().ok()
        } else {
            None
        }
    }

    /// 检查是否可以执行 Dream（考虑时间间隔）。
    pub fn can_execute(&self, config: &DreamConfig) -> bool {
        match self.get_lock_content() {
            None => true,
            Some(lock) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                let hours_since_last = (now - lock.timestamp) as f64 / 3600.0;
                hours_since_last >= config.min_hours_between
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_content_serde() {
        let content = LockContent {
            pid: 12345,
            timestamp: 1700000000,
            session_count: 5,
        };
        let json = serde_json::to_string(&content).unwrap();
        let parsed: LockContent = serde_json::from_str(&json).unwrap();

        assert_eq!(content.pid, parsed.pid);
        assert_eq!(content.timestamp, parsed.timestamp);
        assert_eq!(content.session_count, parsed.session_count);
    }

    #[test]
    fn test_lock_acquire_and_release() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());

        let result = lock.try_acquire(10);
        assert!(result.is_ok());
        assert!(result.unwrap());

        let content = lock.get_lock_content();
        assert!(content.is_some());
        let content = content.unwrap();
        assert_eq!(content.session_count, 10);
        assert!(content.pid > 0);

        lock.release().unwrap();
        assert!(lock.get_lock_content().is_none());
    }

    #[test]
    fn test_lock_double_acquire_fails() {
        let dir = tempfile::tempdir().unwrap();
        let lock1 = DreamLock::new(dir.path());
        let lock2 = DreamLock::new(dir.path());

        lock1.try_acquire(5).unwrap();

        let result = lock2.try_acquire(10);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_lock_can_execute_no_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());

        let config = DreamConfig::default();
        assert!(lock.can_execute(&config));
    }

    #[test]
    fn test_lock_can_execute_with_recent_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());

        lock.try_acquire(5).unwrap();

        let config = DreamConfig {
            min_hours_between: 24.0,
            ..Default::default()
        };

        assert!(!lock.can_execute(&config));
    }
}
