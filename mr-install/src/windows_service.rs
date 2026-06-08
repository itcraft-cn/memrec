use anyhow::{Context, Result};
use std::path::Path;

use crate::service::ServiceManager;

const SERVICE_NAME: &str = "MemRecDaemon";

fn startup_script_path(home_dir: &Path) -> std::path::PathBuf {
    home_dir.join("start_memrecd.ps1")
}

fn get_startup_folder() -> Result<std::path::PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup"))
}

fn get_current_user_path() -> Result<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('Path', 'User')",
        ])
        .output()
        .with_context(|| "Failed to query user PATH")?;
    
    if !output.status.success() {
        anyhow::bail!("Failed to query user PATH");
    }
    
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(path)
}

fn add_to_user_path(dir: &Path) -> Result<()> {
    let dir_str = dir.to_string_lossy().to_string();
    let current = get_current_user_path()?;
    
    if current.split(';').any(|p| p.trim() == dir_str) {
        println!("  PATH already contains {}", dir_str);
        return Ok(());
    }
    
    let new_path = if current.is_empty() {
        dir_str
    } else {
        format!("{};{}", current, dir_str)
    };
    
    let ps_cmd = format!(
        "[Environment]::SetEnvironmentVariable('Path', '{}', 'User')",
        new_path.replace("'", "''")
    );
    
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .output()
        .with_context(|| "Failed to set user PATH")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to update PATH: {}", stderr.trim());
    }
    
    println!("  Added {} to user PATH", dir_str);
    
    broadcast_environment_change();
    
    Ok(())
}

fn remove_from_user_path(dir: &Path) -> Result<()> {
    let dir_str = dir.to_string_lossy().to_string();
    let current = get_current_user_path()?;
    
    let parts: Vec<&str> = current.split(';').filter(|p| p.trim() != dir_str).collect();
    let new_path = parts.join(";");
    
    let ps_cmd = format!(
        "[Environment]::SetEnvironmentVariable('Path', '{}', 'User')",
        new_path.replace("'", "''")
    );
    
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .output()
        .with_context(|| "Failed to update user PATH")?;
    
    if !output.status.success() {
        anyhow::bail!("Failed to remove from PATH");
    }
    
    broadcast_environment_change();
    
    Ok(())
}

fn broadcast_environment_change() {
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public class Env{[DllImport(\"user32.dll\",SetLastError=true,CharSet=CharSet.Auto)]public static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,string l,uint f,uint t,out IntPtr r);public static void Broadcast(){IntPtr r;SendMessageTimeout((IntPtr)0xffff,0x001A,UIntPtr.Zero,\"Environment\",0x0002,5000,out r);}}'; [Env]::Broadcast()",
        ])
        .output();
}

fn try_create_service(bin_path: &Path, home_dir: &Path) -> Result<()> {
    let exe_path = bin_path.join("memrecd.exe");
    let exe_str = exe_path.to_string_lossy();
    let log_path = home_dir.join("memrecd.log");
    let log_str = log_path.to_string_lossy();
    
    let output = std::process::Command::new("sc")
        .args([
            "create", SERVICE_NAME,
            "binPath=", &format!("{} --log-file {}", exe_str, log_str),
            "start=", "auto",
            "obj=", "LocalSystem",
        ])
        .output()
        .with_context(|| "Failed to run sc create")?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("sc create failed: {}", stderr.trim());
    }
    
    println!("  Windows service created: {}", SERVICE_NAME);
    
    let desc_output = std::process::Command::new("sc")
        .args(["description", SERVICE_NAME, "MemRec Memory Persistence Daemon"])
        .output();
    
    if desc_output.map(|o| o.status.success()).unwrap_or(false) {
        println!("  Service description set");
    }
    
    Ok(())
}

fn register_startup_fallback(bin_path: &Path, home_dir: &Path) -> Result<()> {
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

pub struct WindowsService;

impl ServiceManager for WindowsService {
    fn name(&self) -> &str {
        "sc"
    }
    
    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()> {
        match try_create_service(bin_path, home_dir) {
            Ok(()) => {
                println!("  Using Windows Service (sc)");
            }
            Err(e) => {
                println!("  Windows Service creation failed: {}", e);
                println!("  Falling back to Startup script ...");
                register_startup_fallback(bin_path, home_dir)?;
                println!("  Using Startup script (fallback)");
            }
        }
        
        add_to_user_path(bin_path)?;
        
        Ok(())
    }
    
    fn start(&self) -> Result<()> {
        if self.is_active() {
            println!("  memrecd is already running");
            return Ok(());
        }
        
        let sc_output = std::process::Command::new("sc")
            .args(["start", SERVICE_NAME])
            .output();
        
        if sc_output.map(|o| o.status.success()).unwrap_or(false) {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if self.is_active() {
                println!("  Service {} started", SERVICE_NAME);
                return Ok(());
            }
        }
        
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?
            .join(".memrec");
        let script_path = startup_script_path(&home);
        
        if script_path.exists() {
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
        }
        
        std::thread::sleep(std::time::Duration::from_secs(2));
        
        if self.is_active() {
            println!("  memrecd is running");
        } else {
            anyhow::bail!("memrecd failed to start");
        }
        
        Ok(())
    }
    
    fn stop(&self) -> Result<()> {
        let _ = std::process::Command::new("sc")
            .args(["stop", SERVICE_NAME])
            .output();
        
        let _ = std::process::Command::new("taskkill")
            .args(["/IM", "memrecd.exe", "/F"])
            .output();
        
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
        let _ = std::process::Command::new("sc")
            .args(["delete", SERVICE_NAME])
            .output();
        
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
        
        let bin_path = crate::dirs::default_bin_dir();
        remove_from_user_path(&bin_path)?;
        
        Ok(())
    }
}