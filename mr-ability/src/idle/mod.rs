//! # 系统空闲监控模块
//!
//! 提供系统负载检测和空闲判断能力，用于自动触发 Dream 整合作业。
//!
//! ## 平台支持
//!
//! - Linux: 读取 `/proc/loadavg`
//! - macOS: 调用 `sysctl -n vm.loadavg`

mod monitor;

pub use monitor::{DreamRequest, IdleMonitorError, SystemIdleMonitor};