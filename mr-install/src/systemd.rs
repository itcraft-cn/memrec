use anyhow::{Context, Result};
use std::path::PathBuf;

const SERVICE_NAME: &str = "memrecd";

fn service_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join(".config/systemd/user/memrecd.service"))
}

fn bin_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?
        .join(".local/bin"))
}

fn memrec_home() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?
        .join(".memrec"))
}

fn systemctl_is_active() -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", SERVICE_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn systemctl(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run systemctl {}", args.join(" ")))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("systemctl {} failed: {}", args.join(" "), stderr.trim());
    }
    
    Ok(())
}

pub fn register_service() -> Result<()> {
    let service_path = service_file_path()?;
    let bin = bin_dir()?;
    let home = memrec_home()?;
    
    if let Some(parent) = service_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    let service_content = format!(
        r#"[Unit]
Description=MemRec Memory Persistence Daemon
Documentation=https://github.com/anomalyco/memrec
After=default.target

[Service]
Type=simple
ExecStart={bin}/memrecd
ExecStopPost=/bin/rm -f {home}/memrecd.sock
Restart=on-failure
RestartSec=5

Environment="RUST_LOG=info"

WorkingDirectory={home}

StandardOutput=append:{home}/memrecd.log
StandardError=append:{home}/memrecd.log

[Install]
WantedBy=default.target
"#,
        bin = bin.display(),
        home = home.display(),
    );
    
    std::fs::write(&service_path, &service_content)?;
    println!("  Service file: {}", service_path.display());
    
    if systemctl_is_active() {
        println!("  Stopping existing service...");
        systemctl(&["--user", "stop", SERVICE_NAME])?;
    }
    
    systemctl(&["--user", "daemon-reload"])?;
    systemctl(&["--user", "enable", SERVICE_NAME])?;
    systemctl(&["--user", "start", SERVICE_NAME])?;
    
    std::thread::sleep(std::time::Duration::from_secs(2));
    
    if systemctl_is_active() {
        let pid_output = std::process::Command::new("systemctl")
            .args(["--user", "show", "-p", "MainPID", "--value", SERVICE_NAME])
            .output()?;
        let pid = String::from_utf8_lossy(&pid_output.stdout).trim().to_string();
        println!("  Service {} is running (PID: {})", SERVICE_NAME, pid);
    } else {
        anyhow::bail!(
            "Service failed to start. Check logs:\n  systemctl --user status {}\n  cat {}/memrecd.log",
            SERVICE_NAME,
            home.display()
        );
    }
    
    Ok(())
}
