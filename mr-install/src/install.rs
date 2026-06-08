use anyhow::{Context, Result};
use std::path::PathBuf;

const REPO_URL: &str = "https://github.com/itcraft-cn/memrec";

pub struct InstallOptions {
    pub repo_url: Option<String>,
}

fn cargo_bin() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join(".cargo/bin"))
}

pub fn install_binaries(opts: &InstallOptions) -> Result<PathBuf> {
    let cargo = which_cargo()?;
    println!("  Using cargo: {}", cargo.display());
    
    let repo_url = opts.repo_url.as_deref().unwrap_or(REPO_URL);
    println!("  Repository: {}", repo_url);
    
    let crates = ["memrec-common", "memrecd", "memrec", "mr-install"];
    
    for crate_name in &crates {
        println!("  Installing {} ...", crate_name);
        
        let output = std::process::Command::new(&cargo)
            .args([
                "install",
                "--git", repo_url,
                "--locked",
                crate_name,
            ])
            .output()
            .with_context(|| format!("Failed to run cargo install {}", crate_name))?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("cargo install {} failed: {}", crate_name, stderr.trim());
        }
        
        println!("  [ok] {} installed", crate_name);
    }
    
    let cargo_bin_dir = cargo_bin()?;
    let system_bin_dir = crate::dirs::default_bin_dir();
    
    std::fs::create_dir_all(&system_bin_dir)?;
    
    let binaries = ["memrec", "memrecd", "mr-install"];
    for bin in &binaries {
        let src = cargo_bin_dir.join(bin);
        #[cfg(target_family = "windows")]
        let src = cargo_bin_dir.join(format!("{}.exe", bin));
        
        if src.exists() {
            std::fs::copy(&src, system_bin_dir.join(bin))
                .with_context(|| format!("Failed to copy {} to {}", src.display(), system_bin_dir.display()))?;
            println!("  [ok] {} -> {}", bin, system_bin_dir.display());
        }
    }
    
    Ok(system_bin_dir)
}

fn which_cargo() -> Result<PathBuf> {
    #[cfg(target_family = "unix")]
    {
        let output = std::process::Command::new("which")
            .arg("cargo")
            .output()?;
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(PathBuf::from(path));
        }
    }
    
    #[cfg(target_family = "windows")]
    {
        let output = std::process::Command::new("where")
            .arg("cargo")
            .output()?;
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("").trim().to_string();
            return Ok(PathBuf::from(path));
        }
    }
    
    anyhow::bail!("cargo not found. Install Rust first: https://rustup.rs")
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_repo_url() {
        assert_eq!(REPO_URL, "https://github.com/itcraft-cn/memrec");
    }
    
    #[test]
    fn test_cargo_bin_path() {
        let path = cargo_bin().unwrap();
        assert!(path.to_string_lossy().contains(".cargo/bin"));
    }
    
    #[test]
    fn test_install_options_default() {
        let opts = InstallOptions { repo_url: None };
        assert!(opts.repo_url.is_none());
    }
}