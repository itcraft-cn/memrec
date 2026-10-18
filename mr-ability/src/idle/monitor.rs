//! # SystemIdleMonitor 系统空闲监控器
//!
//! 定时检查系统负载，检测到空闲时触发 Dream 作业。
//!
//! ## 工作流程
//!
//! 1. 按配置间隔定期检查系统 load average
//! 2. 当 load 低于阈值时判定为空闲
//! 3. 空闲时向 Dream Queue 发送请求

use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::info;

use mr_common::types::IdleMonitorConfig;

#[derive(Debug, Error)]
pub enum IdleMonitorError {
    #[error("Failed to read system load: {0}")]
    LoadRead(String),
    #[error("Failed to parse load average: {0}")]
    Parse(String),
}

/// Dream 请求消息。
#[derive(Debug, Clone)]
pub struct DreamRequest {
    pub force: bool,
}

/// 获取系统 1分钟 load average。
///
/// Linux 读取 /proc/loadavg，macOS 调用 sysctl。
pub fn get_system_load_1min() -> Result<f64, IdleMonitorError> {
    #[cfg(target_os = "linux")]
    {
        let content = fs::read_to_string("/proc/loadavg")
            .map_err(|e| IdleMonitorError::LoadRead(e.to_string()))?;

        let load_str = content
            .split_whitespace()
            .next()
            .ok_or_else(|| IdleMonitorError::Parse("No load value found".to_string()))?;

        load_str
            .parse::<f64>()
            .map_err(|e| IdleMonitorError::Parse(e.to_string()))
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("sysctl")
            .arg("-n")
            .arg("vm.loadavg")
            .output()
            .map_err(|e| IdleMonitorError::LoadRead(e.to_string()))?;

        let content = String::from_utf8_lossy(&output.stdout);
        let load_str = content.trim().replace("{ ", "").replace(" }", "");

        load_str
            .split_whitespace()
            .next()
            .ok_or_else(|| IdleMonitorError::Parse("No load value found".to_string()))?
            .parse::<f64>()
            .map_err(|e| IdleMonitorError::Parse(e.to_string()))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(IdleMonitorError::LoadRead(
            "Unsupported platform".to_string(),
        ))
    }
}

/// 系统空闲监控器。
///
/// 定时检查系统负载，检测到空闲时向 Dream Queue 发送请求。
pub struct SystemIdleMonitor {
    config: IdleMonitorConfig,
    dream_sender: mpsc::Sender<DreamRequest>,
}

impl SystemIdleMonitor {
    /// 创建新的空闲监控器。
    pub fn new(config: IdleMonitorConfig, dream_sender: mpsc::Sender<DreamRequest>) -> Self {
        Self {
            config,
            dream_sender,
        }
    }

    /// 运行监控循环。
    ///
    /// 按配置间隔检查系统负载，空闲时发送 Dream 请求。
    pub async fn run(self) {
        if !self.config.enabled || !self.config.auto_dream_enabled {
            info!("IdleMonitor disabled, exiting");
            return;
        }

        let mut ticker = interval(Duration::from_secs(self.config.check_interval_secs));

        info!(
            "IdleMonitor started: interval={}s, threshold={}",
            self.config.check_interval_secs, self.config.load_threshold
        );

        loop {
            ticker.tick().await;

            match get_system_load_1min() {
                Ok(load) => {
                    if load < self.config.load_threshold {
                        // 触发通告降为 debug：是否真正执行由 dream_loop 的结果决定，
                        // 此处仅表示"已投递请求"，默认 INFO 级下静默以免空闲期刷屏。
                        tracing::debug!(
                            target: "dream",
                            "IdleMonitor: system load {:.2} < threshold {:.2}, triggering dream",
                            load, self.config.load_threshold
                        );

                        let request = DreamRequest { force: false };
                        if let Err(e) = self.dream_sender.send(request).await {
                            tracing::error!("Failed to send dream request: {}", e);
                        }
                    } else {
                        tracing::debug!(
                            "IdleMonitor: system load {:.2} >= threshold {:.2}, skipping",
                            load,
                            self.config.load_threshold
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to get system load: {}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_system_load_1min() {
        let result = get_system_load_1min();

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            assert!(result.is_ok());
            let load = result.unwrap();
            assert!(load >= 0.0);
        }
    }

    #[tokio::test]
    async fn test_idle_monitor_disabled() {
        let (tx, mut rx) = mpsc::channel::<DreamRequest>(10);
        let config = IdleMonitorConfig {
            enabled: false,
            ..Default::default()
        };

        let monitor = SystemIdleMonitor::new(config, tx);

        tokio::spawn(async move {
            monitor.run().await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_idle_monitor_auto_dream_disabled() {
        let (tx, mut rx) = mpsc::channel::<DreamRequest>(10);
        let config = IdleMonitorConfig {
            auto_dream_enabled: false,
            ..Default::default()
        };

        let monitor = SystemIdleMonitor::new(config, tx);

        tokio::spawn(async move {
            monitor.run().await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(rx.try_recv().is_err());
    }
}
