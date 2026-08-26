//! # DreamLock 并发控制
//!
//! 防止多个 Dream 进程并发执行，导致重复整合。
//!
//! ## 锁文件
//!
//! - 路径：`~/.memrec/dream.lock`
//! - 内容：JSON 格式的 PID、时间戳、会话计数
//! - 仅用于并发控制：获取时写入，释放时删除
//!
//! ## 状态文件
//!
//! - 路径：`~/.memrec/dream.state`
//! - 内容：JSON 格式的上次执行时间戳、会话计数
//! - 用于 DreamGate 时间门槛检查；与锁分离，释放锁后时间信息不丢失
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
///
/// 仅用于并发控制，时间门槛检查请使用 [`DreamState`]。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockContent {
    /// 持有锁的进程 PID
    pub pid: u32,
    /// 获取锁的时间戳（Unix timestamp, 秒）
    pub timestamp: i64,
    /// 会话计数（可选，用于 DreamGate 检查）
    pub session_count: u32,
}

/// Dream 执行状态（持久化到 `dream.state`）。
///
/// 记录上次执行完成的时间戳与会话计数，供 DreamGate 时间门槛检查。
/// 与并发锁（`dream.lock`）分离：锁文件只管并发，释放后不丢失时间信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamState {
    /// 上次执行完成的时间戳（Unix timestamp, 秒）
    pub last_run_at: i64,
    /// 上次执行时的会话计数
    pub session_count: u32,
}

/// DreamLock 并发控制。
pub struct DreamLock {
    lock_path: PathBuf,
    state_path: PathBuf,
}

impl DreamLock {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            lock_path: data_dir.join("dream.lock"),
            state_path: data_dir.join("dream.state"),
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

    /// 读取上次执行状态（不存在返回 `None`）。
    pub fn read_state(&self) -> Option<DreamState> {
        if !self.state_path.exists() {
            return None;
        }

        let mut file = fs::File::open(&self.state_path).ok()?;
        let mut content = String::new();
        file.read_to_string(&mut content).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// 写入执行状态（记录上次执行完成时间戳）。
    pub fn write_state(&self, session_count: u32) -> Result<(), DreamLockError> {
        let state = DreamState {
            last_run_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            session_count,
        };

        let json = serde_json::to_string(&state)?;
        let mut file = fs::File::create(&self.state_path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
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
    fn test_state_serde_roundtrip() {
        let state = DreamState {
            last_run_at: 1700000000,
            session_count: 5,
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: DreamState = serde_json::from_str(&json).unwrap();

        assert_eq!(state.last_run_at, parsed.last_run_at);
        assert_eq!(state.session_count, parsed.session_count);
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
    fn test_state_persists_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());

        assert!(lock.read_state().is_none());

        lock.try_acquire(7).unwrap();
        lock.write_state(7).unwrap();
        lock.release().unwrap();

        // 锁释放后，状态文件仍保留，时间门槛检查不失效
        let state = lock.read_state();
        assert!(state.is_some());
        let state = state.unwrap();
        assert_eq!(state.session_count, 7);
        assert!(state.last_run_at > 0);
    }
}
