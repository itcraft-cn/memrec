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
use tracing::warn;

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
    /// 记忆计数（可选，用于 DreamGate 检查）
    #[serde(alias = "session_count")]
    pub memory_count: u32,
}

/// Dream 执行状态（持久化到 `dream.state`）。
///
/// 记录上次执行完成的时间戳与记忆计数，供 DreamGate 时间门槛检查。
/// 与并发锁（`dream.lock`）分离：锁文件只管并发，释放后不丢失时间信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamState {
    /// 上次执行完成的时间戳（Unix timestamp, 秒）
    pub last_run_at: i64,
    /// 上次执行时的记忆总数
    #[serde(alias = "session_count")]
    pub memory_count: u32,
}

/// 锁守卫：作用域退出时自动释放锁。
///
/// 由 [`DreamLock::try_acquire`] 返回，调用方以 `let _guard = ...` 绑定到
/// 需要互斥的整个作用域，任何提前返回或 panic 都会经 `Drop` 释放锁。
pub struct DreamLockGuard<'a> {
    lock: &'a DreamLock,
}

impl Drop for DreamLockGuard<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.lock.release() {
            warn!(target: "dream", "释放 Dream 锁失败：{e}");
        }
    }
}

/// 当前 Unix 时间戳（秒）；系统时钟异常时退化为 0 而不是 panic。
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

    /// 尝试获取锁，成功时返回 RAII 守卫。
    ///
    /// - `Ok(Some(guard))`：获取成功，`guard` 在 drop 时自动释放锁；
    /// - `Ok(None)`：锁被其他存活进程持有；
    /// - `Err`：IO 或序列化错误。
    ///
    /// ## 为什么必须用守卫
    ///
    /// 旧实现由调用方在函数末尾显式 `release()`。一旦中途 `?` 提前返回或
    /// 发生 panic，锁文件就永久残留；而僵锁检测依据的是「持有进程是否存活」，
    /// 在守护进程内该进程始终存活，于是后续每次获取都失败，Dream 永久停摆。
    /// 守卫把释放绑定到作用域退出，异常路径也能回收。
    ///
    /// ## 独占创建
    ///
    /// 使用 `create_new`（O_EXCL）而非「先 exists 再创建」：后者在并发场景下
    /// 两个调用方会同时判定无锁并各自写入，互斥语义失效。锁文件内容损坏时
    /// 视为僵锁清理（避免一个截断的 JSON 文件永久拒绝 Dream）。
    pub fn try_acquire(
        &self,
        memory_count: u32,
    ) -> Result<Option<DreamLockGuard<'_>>, DreamLockError> {
        // 最多重试 3 次：覆盖「读到僵锁 → 清理 → 被别人抢先创建」的竞争
        for _ in 0..3 {
            if self.write_lock_exclusive(memory_count)? {
                return Ok(Some(DreamLockGuard { lock: self }));
            }

            // 已存在锁文件：判定持有者是否存活
            match self.read_lock() {
                Ok(content) => {
                    if self.is_process_alive(content.pid) {
                        return Ok(None);
                    }
                    warn!(
                        target: "dream",
                        "发现僵锁（pid {} 已退出），清理后重试",
                        content.pid
                    );
                }
                Err(e) => {
                    // 内容损坏（写入中途崩溃）：按僵锁处理，不能让坏文件永久阻塞
                    warn!(target: "dream", "锁文件内容无法解析（{e}），按僵锁清理");
                }
            }

            self.cleanup_stale_lock()?;
        }

        // 连续三次都被抢走，按「已被持有」处理
        Ok(None)
    }

    /// 释放锁（仅当锁仍由本进程持有时删除）。
    ///
    /// 无条件删除会误删别人刚获取的锁：A 的锁被判定为僵锁并由 B 清理后，
    /// B 写入了自己的锁，此时 A 结束并删掉 B 的锁，C 又能获取，互斥失效。
    pub fn release(&self) -> Result<(), DreamLockError> {
        if !self.lock_path.exists() {
            return Ok(());
        }

        match self.read_lock() {
            Ok(content) if content.pid == std::process::id() => {
                fs::remove_file(&self.lock_path)?;
            }
            Ok(content) => {
                warn!(
                    target: "dream",
                    "锁已不属于本进程（持有者 pid {}），跳过释放",
                    content.pid
                );
            }
            Err(e) => {
                warn!(target: "dream", "释放锁时无法解析锁文件（{e}），直接删除");
                fs::remove_file(&self.lock_path)?;
            }
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

    /// 以独占方式创建锁文件。
    ///
    /// @returns `Ok(true)` 创建成功；`Ok(false)` 文件已存在（锁被占用）。
    fn write_lock_exclusive(&self, memory_count: u32) -> Result<bool, DreamLockError> {
        let content = LockContent {
            pid: std::process::id(),
            timestamp: now_unix(),
            memory_count,
        };
        let json = serde_json::to_string(&content)?;

        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.lock_path)
        {
            Ok(mut file) => {
                file.write_all(json.as_bytes())?;
                // 立刻刷盘：避免进程崩溃留下空文件而被后续判定为坏锁
                file.sync_all()?;
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(DreamLockError::Io(e)),
        }
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
    pub fn write_state(&self, memory_count: u32) -> Result<(), DreamLockError> {
        let state = DreamState {
            last_run_at: now_unix(),
            memory_count,
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
            memory_count: 5,
        };
        let json = serde_json::to_string(&content).unwrap();
        let parsed: LockContent = serde_json::from_str(&json).unwrap();

        assert_eq!(content.pid, parsed.pid);
        assert_eq!(content.timestamp, parsed.timestamp);
        assert_eq!(content.memory_count, parsed.memory_count);
    }

    #[test]
    fn test_state_serde_roundtrip() {
        let state = DreamState {
            last_run_at: 1700000000,
            memory_count: 5,
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: DreamState = serde_json::from_str(&json).unwrap();

        assert_eq!(state.last_run_at, parsed.last_run_at);
        assert_eq!(state.memory_count, parsed.memory_count);
    }

    #[test]
    fn test_lock_acquire_and_release() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());

        // 守卫必须绑定到变量：直接 unwrap() 会在语句结束时立即 drop 并释放锁
        let guard = lock.try_acquire(10);
        assert!(guard.is_ok());
        let _guard = guard.unwrap().expect("首次获取应成功");

        let content = lock.get_lock_content();
        assert!(content.is_some());
        let content = content.unwrap();
        assert_eq!(content.memory_count, 10);
        assert!(content.pid > 0);

        lock.release().unwrap();
        assert!(lock.get_lock_content().is_none());
    }

    #[test]
    fn test_lock_double_acquire_fails() {
        let dir = tempfile::tempdir().unwrap();
        let lock1 = DreamLock::new(dir.path());
        let lock2 = DreamLock::new(dir.path());

        let _guard1 = lock1.try_acquire(5).unwrap().expect("首次获取应成功");

        let result = lock2.try_acquire(10);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "同一进程内重复获取应失败");
    }

    #[test]
    fn test_state_persists_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());

        assert!(lock.read_state().is_none());

        let guard = lock.try_acquire(7).unwrap();
        lock.write_state(7).unwrap();
        drop(guard); // RAII 释放

        // 锁释放后，状态文件仍保留，时间门槛检查不失效
        let state = lock.read_state();
        assert!(state.is_some());
        let state = state.unwrap();
        assert_eq!(state.memory_count, 7);
        assert!(state.last_run_at > 0);
    }

    /// 守卫 drop 后锁必须释放（回归：提前返回/panic 导致锁残留）。
    #[test]
    fn test_guard_drop_releases_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());

        {
            let guard = lock.try_acquire(3).unwrap();
            assert!(guard.is_some());
            assert!(lock.get_lock_content().is_some(), "持有期间应有锁文件");
        }

        assert!(
            lock.get_lock_content().is_none(),
            "守卫离开作用域后锁必须被释放"
        );
    }

    /// 锁文件内容损坏时必须按僵锁清理，而不是永久拒绝 Dream。
    #[test]
    fn test_corrupt_lock_file_is_treated_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());
        std::fs::write(dir.path().join("dream.lock"), b"{\"pid\":").unwrap();

        let guard = lock.try_acquire(1).expect("坏锁不应导致 Err");
        assert!(guard.is_some(), "坏锁应被清理并成功获取");
    }

    /// 释放时若锁已被他人接管，不得删除对方的锁。
    #[test]
    fn test_release_skips_lock_owned_by_another_process() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DreamLock::new(dir.path());
        let _guard = lock.try_acquire(2).unwrap().expect("首次获取应成功");

        // 模拟锁被另一进程接管
        let other = LockContent {
            pid: 1,
            timestamp: now_unix(),
            memory_count: 2,
        };
        std::fs::write(
            dir.path().join("dream.lock"),
            serde_json::to_string(&other).unwrap(),
        )
        .unwrap();

        lock.release().unwrap();

        let content = lock.get_lock_content().expect("他人持有的锁不应被删除");
        assert_eq!(content.pid, 1);
    }
}
