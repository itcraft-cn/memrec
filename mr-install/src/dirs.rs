use anyhow::Result;
use std::path::PathBuf;

pub fn memrec_home() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join(".memrec"))
}

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
    
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir()
            .map(|d| d.join("memrec"))
            .unwrap_or_else(|| std::path::PathBuf::from("C:\\ProgramData\\memrec"))
    }
}

pub fn create_directories() -> Result<PathBuf> {
    let home = memrec_home()?;
    create_directories_at(&home)?;
    Ok(home)
}

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
