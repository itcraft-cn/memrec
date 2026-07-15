//! # 目录管理
//!
//! 创建 `~/.memrec/` 及其子目录（data、vectors、models、logs）。

use anyhow::Result;
use std::path::PathBuf;

/// 返回 `~/.memrec/` 路径
pub fn memrec_home() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join(".memrec"))
}

/// 默认二进制目录：Linux `~/.local/bin`，macOS `~/bin`
pub fn default_bin_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs::home_dir()
            .map(|h| h.join(".local/bin"))
            .unwrap_or_else(|| std::path::PathBuf::from("/usr/local/bin"))
    }

    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .map(|h| h.join("bin"))
            .unwrap_or_else(|| std::path::PathBuf::from("/usr/local/bin"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        compile_error!("Unsupported platform. Only Linux and macOS are supported.");
    }
}

/// 创建 `~/.memrec/` 及子目录
pub fn create_directories() -> Result<PathBuf> {
    let home = memrec_home()?;
    create_directories_at(&home)?;
    Ok(home)
}

/// 在指定基路径下创建 data/vectors/models/logs 子目录
pub fn create_directories_at(base: &std::path::Path) -> Result<()> {
    let dirs_to_create = [
        base.join("data"),
        base.join("vectors"),
        base.join("models"),
        base.join("logs"),
    ];

    for dir in &dirs_to_create {
        std::fs::create_dir_all(dir)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_memrec_home() {
        let home = memrec_home().unwrap();
        assert!(home.to_string_lossy().ends_with(".memrec"));
    }

    #[test]
    fn test_create_directories_at() {
        let dir = tempdir().unwrap();
        create_directories_at(dir.path()).unwrap();

        assert!(dir.path().join("data").exists());
        assert!(dir.path().join("vectors").exists());
        assert!(dir.path().join("models").exists());
        assert!(dir.path().join("logs").exists());
    }
}
