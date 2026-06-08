use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::service::ServiceManager;

const LABEL: &str = "com.itcraft.memrecd";

fn plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join("Library/LaunchAgents").join(format!("{}.plist", LABEL)))
}

fn launchctl(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run launchctl {}", args.join(" ")))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("launchctl {} failed: {}", args.join(" "), stderr.trim());
    }
    
    Ok(())
}

fn get_uid() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| {
            std::env::var("UID").unwrap_or_else(|_| "501".to_string())
        })
}

pub struct LaunchdService;

impl ServiceManager for LaunchdService {
    fn name(&self) -> &str {
        "launchd"
    }
    
    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()> {
        let plist = plist_path()?;
        
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let log_path = home_dir.join("memrecd.log");
        
        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}/memrecd</string>
    </array>
    <key>WorkingDirectory</key>
    <string>{home}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>SoftResourceLimits</key>
    <dict>
        <key>NumberOfFiles</key>
        <integer>4096</integer>
    </dict>
</dict>
</plist>
"#,
            label = LABEL,
            bin = bin_path.display(),
            home = home_dir.display(),
            log = log_path.display(),
        );
        
        std::fs::write(&plist, &plist_content)?;
        println!("  LaunchAgent plist: {}", plist.display());
        
        Ok(())
    }
    
    fn start(&self) -> Result<()> {
        let uid = get_uid();
        let plist_str = format!("~/Library/LaunchAgents/{}.plist", LABEL);
        let gui_target = format!("gui/{}", uid);
        
        if self.is_active() {
            launchctl(&["bootout", &gui_target, &plist_str])?;
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        
        launchctl(&["bootstrap", &gui_target, &plist_str])?;
        
        std::thread::sleep(std::time::Duration::from_secs(2));
        
        if self.is_active() {
            println!("  LaunchAgent {} is running", LABEL);
        } else {
            anyhow::bail!("LaunchAgent failed to start. Check: launchctl list {}", LABEL);
        }
        
        Ok(())
    }
    
    fn stop(&self) -> Result<()> {
        if self.is_active() {
            let uid = get_uid();
            let plist_str = format!("~/Library/LaunchAgents/{}.plist", LABEL);
            let gui_target = format!("gui/{}", uid);
            launchctl(&["bootout", &gui_target, &plist_str])?;
        }
        Ok(())
    }
    
    fn is_active(&self) -> bool {
        std::process::Command::new("launchctl")
            .args(["list", LABEL])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    
    fn unregister(&self) -> Result<()> {
        self.stop()?;
        
        let plist = plist_path()?;
        if plist.exists() {
            std::fs::remove_file(&plist)?;
        }
        
        Ok(())
    }
}
