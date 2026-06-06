use anyhow::Result;
use std::path::PathBuf;

pub fn memrec_home() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join(".memrec"))
}

pub fn create_directories() -> Result<PathBuf> {
    let home = memrec_home()?;
    
    let dirs_to_create = [
        home.join("data"),
        home.join("vectors"),
        home.join("models"),
        home.join("logs"),
    ];
    
    for dir in &dirs_to_create {
        std::fs::create_dir_all(dir)?;
    }
    
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
