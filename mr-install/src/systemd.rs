use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::service::ServiceManager;

const SERVICE_NAME: &str = "memrecd";

fn service_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join(".config/systemd/user/memrecd.service"))
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

pub struct SystemdService;

impl ServiceManager for SystemdService {
    fn name(&self) -> &str {
        "systemd"
    }

    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()> {
        let service_path = service_file_path()?;

        if let Some(parent) = service_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let service_content = format!(
            r#"[Unit]
Description=MemRec Memory Persistence Daemon
Documentation=https://github.com/itcraft-cn/memrec
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
            bin = bin_path.display(),
            home = home_dir.display(),
        );

        std::fs::write(&service_path, &service_content)?;
        println!("  Service file: {}", service_path.display());

        Ok(())
    }

    fn start(&self) -> Result<()> {
        if self.is_active() {
            systemctl(&["--user", "stop", SERVICE_NAME])?;
        }

        systemctl(&["--user", "daemon-reload"])?;
        systemctl(&["--user", "enable", SERVICE_NAME])?;
        systemctl(&["--user", "start", SERVICE_NAME])?;

        std::thread::sleep(std::time::Duration::from_secs(2));

        if self.is_active() {
            let pid_output = std::process::Command::new("systemctl")
                .args(["--user", "show", "-p", "MainPID", "--value", SERVICE_NAME])
                .output()?;
            let pid = String::from_utf8_lossy(&pid_output.stdout)
                .trim()
                .to_string();
            println!("  Service {} is running (PID: {})", SERVICE_NAME, pid);
        } else {
            anyhow::bail!(
                "Service failed to start. Check: systemctl --user status {}",
                SERVICE_NAME
            );
        }

        Ok(())
    }

    fn stop(&self) -> Result<()> {
        if self.is_active() {
            systemctl(&["--user", "stop", SERVICE_NAME])?;
        }
        Ok(())
    }

    fn is_active(&self) -> bool {
        std::process::Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", SERVICE_NAME])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn unregister(&self) -> Result<()> {
        self.stop()?;
        systemctl(&["--user", "disable", SERVICE_NAME])?;

        let service_path = service_file_path()?;
        if service_path.exists() {
            std::fs::remove_file(&service_path)?;
        }

        systemctl(&["--user", "daemon-reload"])?;
        Ok(())
    }
}
