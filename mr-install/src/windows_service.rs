use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::service::ServiceManager;

const TASK_NAME: &str = "MemRecDaemon";

fn startup_script_path(home_dir: &Path) -> PathBuf {
    home_dir.join("start_memrecd.ps1")
}

fn get_startup_folder() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup"))
}

pub struct WindowsService;

impl ServiceManager for WindowsService {
    fn name(&self) -> &str {
        "schtasks"
    }
    
    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()> {
        let script_path = startup_script_path(home_dir);
        
        let script_content = format!(
            r#"$memrecd = "{bin}\memrecd.exe"
$log = "{home}\memrecd.log"

if (Test-Path $memrecd) {{
    Start-Process -FilePath $memrecd -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $log
}} else {{
    Write-Error "memrecd.exe not found at $memrecd"
}}
"#,
            bin = bin_path.display(),
            home = home_dir.display(),
        );
        
        std::fs::write(&script_path, &script_content)?;
        println!("  Startup script: {}", script_path.display());
        
        let startup_folder = get_startup_folder()?;
        std::fs::create_dir_all(&startup_folder)?;
        
        let vbs_path = startup_folder.join("memrecd.vbs");
        let vbs_content = format!(
            r#"Set WshShell = CreateObject("WScript.Shell")
WshShell.Run "powershell -WindowStyle Hidden -ExecutionPolicy Bypass -File ""{}""", 0, False
"#,
            script_path.display(),
        );
        
        std::fs::write(&vbs_path, vbs_content)?;
        println!("  Startup shortcut: {}", vbs_path.display());
        
        Ok(())
    }
    
    fn start(&self) -> Result<()> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?
            .join(".memrec");
        let script_path = startup_script_path(&home);
        
        let output = std::process::Command::new("powershell")
            .args([
                "-WindowStyle", "Hidden",
                "-ExecutionPolicy", "Bypass",
                "-File", &script_path.to_string_lossy(),
            ])
            .output()
            .with_context(|| "Failed to start memrecd via PowerShell")?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to start memrecd: {}", stderr.trim());
        }
        
        std::thread::sleep(std::time::Duration::from_secs(2));
        
        if self.is_active() {
            println!("  memrecd is running");
        } else {
            anyhow::bail!("memrecd failed to start. Check: {}", home.join("memrecd.log").display());
        }
        
        Ok(())
    }
    
    fn stop(&self) -> Result<()> {
        let output = std::process::Command::new("taskkill")
            .args(["/IM", "memrecd.exe", "/F"])
            .output();
        
        match output {
            Ok(o) if o.status.success() => {}
            _ => {}
        }
        
        Ok(())
    }
    
    fn is_active(&self) -> bool {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq memrecd.exe", "/NH"])
            .output()
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains("memrecd.exe")
            })
            .unwrap_or(false)
    }
    
    fn unregister(&self) -> Result<()> {
        self.stop()?;
        
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?
            .join(".memrec");
        
        let script_path = startup_script_path(&home);
        if script_path.exists() {
            std::fs::remove_file(&script_path)?;
        }
        
        let startup_folder = get_startup_folder()?;
        let vbs_path = startup_folder.join("memrecd.vbs");
        if vbs_path.exists() {
            std::fs::remove_file(&vbs_path)?;
        }
        
        Ok(())
    }
}
