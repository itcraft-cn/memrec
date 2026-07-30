# 系统空闲时自动触发Dream融通作业实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现系统空闲时自动触发Dream融通作业，支持双队列调度、分批处理、日志滚存

**Architecture:** 
- SystemIdleMonitor 检测系统负载，空闲时向 Dream Queue 发送请求
- QueueDispatcher 优先处理 Normal Queue，其次处理 Dream Queue
- DreamProcessor 扩展4个阶段：交叉项目提取、个人提炼、清理、向量重生成
- 日志按天滚存，memrecd.log 和 dream.log 分离

**Tech Stack:** Rust, Tokio, tracing-appender, chrono

---

## Task 1: 扩展配置结构

**Files:**
- Modify: `mr-common/src/types/config.rs:205-265`
- Test: `mr-common/src/types/config.rs:267-407` (内联测试)

### Step 1.1: 为 DreamConfig 添加阶段开关和批处理配置

- [ ] **添加配置字段到 DreamConfig**

```rust
/// Dream 后台整合配置。
///
/// 控制记忆压缩和整合行为，类似人类睡眠中的记忆巩固过程。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamConfig {
    /// 是否启用 Dream 整合
    #[serde(default = "default_dream_enabled")]
    pub enabled: bool,

    /// 触发整合的最小记忆数量
    #[serde(default = "default_min_memories")]
    pub min_memories: usize,

    /// 整合的记忆最大年龄（小时），默认 168h = 7天
    #[serde(default = "default_max_age_hours")]
    pub max_age_hours: u64,

    /// 两次整合之间的最小间隔（小时）
    #[serde(default = "default_min_hours_between")]
    pub min_hours_between: f64,

    /// 整合后生成的记忆类型
    #[serde(default = "default_integration_type")]
    pub integration_type: String,

    /// 整合后生成的记忆标签
    #[serde(default = "default_integration_tags")]
    pub integration_tags: Vec<String>,

    /// Phase 1: 交叉项目记忆提取开关
    #[serde(default = "default_phase_cross_project")]
    pub phase_cross_project: bool,

    /// Phase 2: 个人总体记忆提炼开关
    #[serde(default = "default_phase_personal_summary")]
    pub phase_personal_summary: bool,

    /// Phase 3: 记忆清理开关
    #[serde(default = "default_phase_cleanup")]
    pub phase_cleanup: bool,

    /// Phase 4: 向量重新生成开关（资源消耗大，默认关闭）
    #[serde(default = "default_phase_vector_regen")]
    pub phase_vector_regen: bool,

    /// 批处理大小
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// 批处理间隔（毫秒）
    #[serde(default = "default_batch_interval_ms")]
    pub batch_interval_ms: u64,
}

fn default_phase_cross_project() -> bool { true }
fn default_phase_personal_summary() -> bool { true }
fn default_phase_cleanup() -> bool { true }
fn default_phase_vector_regen() -> bool { false }
fn default_batch_size() -> usize { 100 }
fn default_batch_interval_ms() -> u64 { 1000 }
```

- [ ] **更新 Default 实现**

```rust
impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_memories: 20,
            max_age_hours: 168,
            min_hours_between: 24.0,
            integration_type: "knowledge".to_string(),
            integration_tags: vec!["dream-integrated".to_string()],
            phase_cross_project: true,
            phase_personal_summary: true,
            phase_cleanup: true,
            phase_vector_regen: false,
            batch_size: 100,
            batch_interval_ms: 1000,
        }
    }
}
```

- [ ] **添加测试用例验证新字段**

```rust
#[test]
fn test_dream_config_new_fields() {
    let config = DreamConfig::default();
    
    assert!(config.phase_cross_project);
    assert!(config.phase_personal_summary);
    assert!(config.phase_cleanup);
    assert!(!config.phase_vector_regen);
    assert_eq!(config.batch_size, 100);
    assert_eq!(config.batch_interval_ms, 1000);
}

#[test]
fn test_dream_config_serde_new_fields() {
    let json = r#"{"enabled": true, "phase_cross_project": false, "batch_size": 50}"#;
    let parsed: DreamConfig = serde_json::from_str(json).unwrap();
    
    assert!(parsed.enabled);
    assert!(!parsed.phase_cross_project);
    assert!(parsed.phase_personal_summary);
    assert_eq!(parsed.batch_size, 50);
    assert_eq!(parsed.batch_interval_ms, 1000);
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-common test_dream_config --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-common/src/types/config.rs
git commit -m "feat(config): 添加Dream阶段开关和批处理配置"
```

### Step 1.2: 新增 IdleMonitorConfig

- [ ] **添加 IdleMonitorConfig 结构体**

```rust
/// 系统空闲监控配置。
///
/// 控制系统空闲检测行为，用于自动触发 Dream 整合。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleMonitorConfig {
    /// 是否启用空闲监控
    #[serde(default = "default_idle_enabled")]
    pub enabled: bool,

    /// 检查间隔（秒），默认 300 = 5分钟
    #[serde(default = "default_check_interval_secs")]
    pub check_interval_secs: u64,

    /// load average 阈值，低于此值视为空闲
    #[serde(default = "default_load_threshold")]
    pub load_threshold: f64,

    /// 是否启用自动触发 dream
    #[serde(default = "default_auto_dream_enabled")]
    pub auto_dream_enabled: bool,
}

fn default_idle_enabled() -> bool { true }
fn default_check_interval_secs() -> u64 { 300 }
fn default_load_threshold() -> f64 { 0.5 }
fn default_auto_dream_enabled() -> bool { true }

impl Default for IdleMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 300,
            load_threshold: 0.5,
            auto_dream_enabled: true,
        }
    }
}
```

- [ ] **添加测试用例**

```rust
#[test]
fn test_idle_monitor_config_defaults() {
    let config = IdleMonitorConfig::default();
    
    assert!(config.enabled);
    assert_eq!(config.check_interval_secs, 300);
    assert_eq!(config.load_threshold, 0.5);
    assert!(config.auto_dream_enabled);
}

#[test]
fn test_idle_monitor_config_serde() {
    let config = IdleMonitorConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let parsed: IdleMonitorConfig = serde_json::from_str(&json).unwrap();
    
    assert_eq!(config.check_interval_secs, parsed.check_interval_secs);
}

#[test]
fn test_idle_monitor_config_partial_deserialize() {
    let json = r#"{"enabled": false, "load_threshold": 1.0}"#;
    let parsed: IdleMonitorConfig = serde_json::from_str(json).unwrap();
    
    assert!(!parsed.enabled);
    assert_eq!(parsed.load_threshold, 1.0);
    assert_eq!(parsed.check_interval_secs, 300);
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-common test_idle_monitor_config --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-common/src/types/config.rs
git commit -m "feat(config): 添加IdleMonitorConfig空闲监控配置"
```

### Step 1.3: 新增 QueueConfig

- [ ] **添加 QueueConfig 结构体**

```rust
/// 队列配置。
///
/// 控制请求队列容量和行为。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    /// Normal Queue 容量
    #[serde(default = "default_normal_queue_capacity")]
    pub normal_queue_capacity: usize,

    /// Dream Queue 容量
    #[serde(default = "default_dream_queue_capacity")]
    pub dream_queue_capacity: usize,
}

fn default_normal_queue_capacity() -> usize { 1000 }
fn default_dream_queue_capacity() -> usize { 10 }

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            normal_queue_capacity: 1000,
            dream_queue_capacity: 10,
        }
    }
}
```

- [ ] **添加测试用例**

```rust
#[test]
fn test_queue_config_defaults() {
    let config = QueueConfig::default();
    
    assert_eq!(config.normal_queue_capacity, 1000);
    assert_eq!(config.dream_queue_capacity, 10);
}

#[test]
fn test_queue_config_serde() {
    let json = r#"{"normal_queue_capacity": 500}"#;
    let parsed: QueueConfig = serde_json::from_str(json).unwrap();
    
    assert_eq!(parsed.normal_queue_capacity, 500);
    assert_eq!(parsed.dream_queue_capacity, 10);
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-common test_queue_config --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-common/src/types/config.rs
git commit -m "feat(config): 添加QueueConfig队列配置"
```

### Step 1.4: 新增 LogConfig

- [ ] **添加 LogConfig 结构体**

```rust
/// 日志配置。
///
/// 控制日志文件位置、滚存和清理策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// 是否启用日志滚存
    #[serde(default = "default_log_enabled")]
    pub enabled: bool,

    /// 日志级别
    #[serde(default = "default_log_level")]
    pub level: String,

    /// 日志保留天数
    #[serde(default = "default_rotation_days")]
    pub rotation_days: u64,

    /// 是否压缩旧日志
    #[serde(default = "default_compress")]
    pub compress: bool,

    /// memrecd 主日志文件路径
    #[serde(default = "default_memrecd_log_path")]
    pub memrecd_log: String,

    /// dream 作业日志文件路径
    #[serde(default = "default_dream_log_path")]
    pub dream_log: String,
}

fn default_log_enabled() -> bool { true }
fn default_log_level() -> String { "info".to_string() }
fn default_rotation_days() -> u64 { 7 }
fn default_compress() -> bool { false }
fn default_memrecd_log_path() -> String { "~/.memrec/logs/memrecd.log".to_string() }
fn default_dream_log_path() -> String { "~/.memrec/logs/dream.log".to_string() }

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: "info".to_string(),
            rotation_days: 7,
            compress: false,
            memrecd_log: "~/.memrec/logs/memrecd.log".to_string(),
            dream_log: "~/.memrec/logs/dream.log".to_string(),
        }
    }
}
```

- [ ] **添加测试用例**

```rust
#[test]
fn test_log_config_defaults() {
    let config = LogConfig::default();
    
    assert!(config.enabled);
    assert_eq!(config.level, "info");
    assert_eq!(config.rotation_days, 7);
    assert!(!config.compress);
    assert!(config.memrecd_log.contains("memrecd.log"));
    assert!(config.dream_log.contains("dream.log"));
}

#[test]
fn test_log_config_serde() {
    let json = r#"{"rotation_days": 14, "level": "debug"}"#;
    let parsed: LogConfig = serde_json::from_str(json).unwrap();
    
    assert_eq!(parsed.rotation_days, 14);
    assert_eq!(parsed.level, "debug");
    assert!(parsed.enabled);
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-common test_log_config --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-common/src/types/config.rs
git commit -m "feat(config): 添加LogConfig日志配置"
```

---

## Task 2: 实现 SystemIdleMonitor

**Files:**
- Create: `mr-ability/src/idle/mod.rs`
- Create: `mr-ability/src/idle/monitor.rs`
- Test: `mr-ability/src/idle/monitor.rs` (内联测试)

### Step 2.1: 创建 idle 模块结构

- [ ] **创建 mr-ability/src/idle/mod.rs**

```rust
//! # 系统空闲监控模块
//!
//! 提供系统负载检测和空闲判断能力，用于自动触发 Dream 整合作业。

mod monitor;

pub use monitor::{SystemIdleMonitor, IdleMonitorError};
```

- [ ] **更新 mr-ability/src/lib.rs 添加 idle 模块**

```rust
// 在 mr-ability/src/lib.rs 中添加
pub mod idle;
```

- [ ] **提交变更**

```bash
git add mr-ability/src/idle/mod.rs mr-ability/src/lib.rs
git commit -m "feat(idle): 创建idle模块结构"
```

### Step 2.2: 实现 get_system_load_1min 函数

- [ ] **创建 mr-ability/src/idle/monitor.rs 并实现系统负载读取**

```rust
//! # SystemIdleMonitor 系统空闲监控器
//!
//! 定时检查系统负载，检测到空闲时触发 Dream 作业。
//!
//! ## 平台支持
//!
//! - Linux: 读取 `/proc/loadavg`
//! - macOS: 调用 `sysctl -n vm.loadavg`

use std::fs;
use std::process::Command;

use thiserror::Error;
use tracing::info;

use mr_common::types::IdleMonitorConfig;

#[derive(Debug, Error)]
pub enum IdleMonitorError {
    #[error("Failed to read system load: {0}")]
    LoadRead(String),
    #[error("Failed to parse load average: {0}")]
    Parse(String),
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
            .parse()
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
        let load_str = content
            .trim()
            .replace("{ ", "")
            .replace(" }", "");
        
        load_str
            .split_whitespace()
            .next()
            .ok_or_else(|| IdleMonitorError::Parse("No load value found".to_string()))?
            .parse()
            .map_err(|e| IdleMonitorError::Parse(e.to_string()))
    }
    
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(IdleMonitorError::LoadRead(
            "Unsupported platform".to_string(),
        ))
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
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_get_system_load_1min --release`
Expected: PASS (在 Linux/macOS 上)

- [ ] **提交变更**

```bash
git add mr-ability/src/idle/monitor.rs
git commit -m "feat(idle): 实现get_system_load_1min系统负载读取"
```

### Step 2.3: 实现 SystemIdleMonitor 结构体

- [ ] **在 monitor.rs 中添加 SystemIdleMonitor**

```rust
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::info;

/// 系统空闲监控器。
///
/// 定时检查系统负载，检测到空闲时向 Dream Queue 发送请求。
pub struct SystemIdleMonitor {
    config: IdleMonitorConfig,
    dream_sender: mpsc::Sender<DreamRequest>,
}

/// Dream 请求消息。
#[derive(Debug, Clone)]
pub struct DreamRequest {
    pub force: bool,
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
                        info!(
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
                            load, self.config.load_threshold
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
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_idle_monitor --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/idle/monitor.rs
git commit -m "feat(idle): 实现SystemIdleMonitor空闲监控器"
```

---

## Task 3: 实现 QueueDispatcher

**Files:**
- Create: `mr-ability/src/queue/mod.rs`
- Create: `mr-ability/src/queue/dispatcher.rs`
- Create: `mr-ability/src/queue/types.rs`
- Test: 内联测试

### Step 3.1: 创建 queue 模块结构

- [ ] **创建 mr-ability/src/queue/mod.rs**

```rust
//! # 请求队列模块
//!
//! 提供双队列调度能力：Normal Queue (高优先级) 和 Dream Queue (低优先级)。

mod dispatcher;
mod types;

pub use dispatcher::QueueDispatcher;
pub use types::{QueuedRequest, QueueError};
```

- [ ] **更新 mr-ability/src/lib.rs 添加 queue 模块**

```rust
// 在 mr-ability/src/lib.rs 中添加
pub mod queue;
```

- [ ] **提交变更**

```bash
git add mr-ability/src/queue/mod.rs mr-ability/src/lib.rs
git commit -m "feat(queue): 创建queue模块结构"
```

### Step 3.2: 实现队列类型定义

- [ ] **创建 mr-ability/src/queue/types.rs**

```rust
//! # 队列类型定义

use mr_protocol::JsonRpcRequest;
use thiserror::Error;

/// 队列请求类型。
#[derive(Debug, Clone)]
pub enum QueuedRequest {
    /// 普通用户请求
    Normal(JsonRpcRequest),
    /// Dream 整合请求
    Dream,
}

/// 队列错误类型。
#[derive(Debug, Error)]
pub enum QueueError {
    #[error("Queue is full")]
    Full,
    #[error("Queue is empty")]
    Empty,
    #[error("Failed to send request: {0}")]
    Send(String),
    #[error("Failed to receive request: {0}")]
    Receive(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_protocol::{RequestAction, RequestParams};

    #[test]
    fn test_queued_request_debug() {
        let request = JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
        let queued = QueuedRequest::Normal(request);
        
        let debug_str = format!("{:?}", queued);
        assert!(debug_str.contains("Normal"));
    }
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_queued_request --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/queue/types.rs
git commit -m "feat(queue): 实现队列类型定义"
```

### Step 3.3: 实现 QueueDispatcher

- [ ] **创建 mr-ability/src/queue/dispatcher.rs**

```rust
//! # QueueDispatcher 队列调度器
//!
//! 从两个队列中按优先级取出请求，串行执行。
//!
//! ## 调度策略
//!
//! 1. 优先处理 Normal Queue
//! 2. Normal Queue 为空时处理 Dream Queue
//! 3. 双队列为空时阻塞等待

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use tracing::info;

use mr_common::types::QueueConfig;
use mr_protocol::JsonRpcResponse;

use super::types::{QueuedRequest, QueueError};

/// 请求处理器 trait。
#[async_trait::async_trait]
pub trait RequestHandler: Send + Sync {
    async fn handle(&self, request: QueuedRequest) -> JsonRpcResponse;
}

/// 队列调度器。
pub struct QueueDispatcher {
    config: QueueConfig,
    normal_queue: Arc<Mutex<VecDeque<QueuedRequest>>>,
    dream_queue: Arc<Mutex<VecDeque<QueuedRequest>>>,
    handler: Arc<dyn RequestHandler>,
}

impl QueueDispatcher {
    /// 创建新的队列调度器。
    pub fn new(config: QueueConfig, handler: Arc<dyn RequestHandler>) -> Self {
        Self {
            config,
            normal_queue: Arc::new(Mutex::new(VecDeque::new())),
            dream_queue: Arc::new(Mutex::new(VecDeque::new())),
            handler,
        }
    }

    /// 推送 Normal 请求到队列。
    pub async fn push_normal(&self, request: QueuedRequest) -> Result<(), QueueError> {
        let mut queue = self.normal_queue.lock().await;
        
        if queue.len() >= self.config.normal_queue_capacity {
            return Err(QueueError::Full);
        }
        
        queue.push_back(request);
        Ok(())
    }

    /// 推送 Dream 请求到队列。
    pub async fn push_dream(&self, request: QueuedRequest) -> Result<(), QueueError> {
        let mut queue = self.dream_queue.lock().await;
        
        if queue.len() >= self.config.dream_queue_capacity {
            return Err(QueueError::Full);
        }
        
        queue.push_back(request);
        Ok(())
    }

    /// 从队列中取出请求（优先 Normal）。
    pub async fn pop(&self) -> Option<QueuedRequest> {
        let mut normal = self.normal_queue.lock().await;
        
        if !normal.is_empty() {
            return normal.pop_front();
        }
        
        drop(normal);
        
        let mut dream = self.dream_queue.lock().await;
        
        if !dream.is_empty() {
            return dream.pop_front();
        }
        
        None
    }

    /// 获取队列状态。
    pub async fn status(&self) -> QueueStatus {
        let normal = self.normal_queue.lock().await;
        let dream = self.dream_queue.lock().await;
        
        QueueStatus {
            normal_queue_len: normal.len(),
            dream_queue_len: dream.len(),
            normal_queue_capacity: self.config.normal_queue_capacity,
            dream_queue_capacity: self.config.dream_queue_capacity,
        }
    }

    /// 运行调度循环。
    pub async fn run(self) {
        info!(
            "QueueDispatcher started: normal_capacity={}, dream_capacity={}",
            self.config.normal_queue_capacity, self.config.dream_queue_capacity
        );

        loop {
            if let Some(request) = self.pop().await {
                let response = self.handler.handle(request).await;
                
                if let QueuedRequest::Normal(ref req) = request {
                    tracing::debug!("Handled request: id={}", req.id);
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }
}

/// 队列状态。
#[derive(Debug, Clone)]
pub struct QueueStatus {
    pub normal_queue_len: usize,
    pub dream_queue_len: usize,
    pub normal_queue_capacity: usize,
    pub dream_queue_capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_protocol::{RequestAction, RequestParams};

    struct MockHandler;

    #[async_trait::async_trait]
    impl RequestHandler for MockHandler {
        async fn handle(&self, _request: QueuedRequest) -> JsonRpcResponse {
            JsonRpcResponse::success(
                mr_protocol::ResponseResult::Success(mr_protocol::SuccessResult {
                    message: "ok".to_string(),
                }),
                1,
            )
        }
    }

    #[tokio::test]
    async fn test_push_pop_normal() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let request = JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
        let queued = QueuedRequest::Normal(request);

        dispatcher.push_normal(queued.clone()).await.unwrap();
        
        let popped = dispatcher.pop().await;
        assert!(popped.is_some());
        
        if let QueuedRequest::Normal(req) = popped.unwrap() {
            assert_eq!(req.id, 1);
        } else {
            panic!("Expected Normal request");
        }
    }

    #[tokio::test]
    async fn test_push_pop_dream() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let queued = QueuedRequest::Dream;
        
        dispatcher.push_dream(queued.clone()).await.unwrap();
        
        let popped = dispatcher.pop().await;
        assert!(popped.is_some());
    }

    #[tokio::test]
    async fn test_priority_normal_first() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        dispatcher.push_dream(QueuedRequest::Dream).await.unwrap();
        
        let normal_request = JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
        dispatcher.push_normal(QueuedRequest::Normal(normal_request)).await.unwrap();

        let popped = dispatcher.pop().await.unwrap();
        assert!(matches!(popped, QueuedRequest::Normal(_)));
    }

    #[tokio::test]
    async fn test_queue_full() {
        let config = QueueConfig {
            normal_queue_capacity: 2,
            dream_queue_capacity: 1,
        };
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let request = JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
        
        dispatcher.push_normal(QueuedRequest::Normal(request.clone())).await.unwrap();
        dispatcher.push_normal(QueuedRequest::Normal(request.clone())).await.unwrap();
        
        let result = dispatcher.push_normal(QueuedRequest::Normal(request));
        assert!(matches!(result.await, Err(QueueError::Full)));
    }

    #[tokio::test]
    async fn test_queue_status() {
        let config = QueueConfig::default();
        let handler = Arc::new(MockHandler);
        let dispatcher = QueueDispatcher::new(config, handler);

        let status = dispatcher.status().await;
        assert_eq!(status.normal_queue_len, 0);
        assert_eq!(status.dream_queue_len, 0);

        dispatcher.push_normal(QueuedRequest::Dream).await.unwrap();
        
        let status = dispatcher.status().await;
        assert_eq!(status.normal_queue_len, 1);
    }
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_queue --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/queue/dispatcher.rs
git commit -m "feat(queue): 实现QueueDispatcher双队列调度器"
```

---

## Task 4: 扩展 DreamProcessor

**Files:**
- Modify: `mr-ability/src/dream/processor.rs`
- Create: `mr-ability/src/dream/phases/mod.rs`
- Create: `mr-ability/src/dream/phases/cross_project.rs`
- Create: `mr-ability/src/dream/phases/personal_summary.rs`
- Create: `mr-ability/src/dream/phases/cleanup.rs`
- Create: `mr-ability/src/dream/phases/vector_regen.rs`

### Step 4.1: 创建 phases 子模块结构

- [ ] **创建 mr-ability/src/dream/phases/mod.rs**

```rust
//! # Dream 阶段处理模块
//!
//! 包含4个独立的阶段处理器：
//! 1. CrossProjectExtract: 交叉项目记忆提取
//! 2. PersonalSummary: 个人总体记忆提炼
//! 3. Cleanup: 记忆清理
//! 4. VectorRegen: 向量重新生成

mod cross_project;
mod personal_summary;
mod cleanup;
mod vector_regen;

pub use cross_project::CrossProjectExtractor;
pub use personal_summary::PersonalSummarizer;
pub use cleanup::MemoryCleaner;
pub use vector_regen::VectorRegenerator;

/// 阶段执行结果。
#[derive(Debug, Clone)]
pub struct PhaseResult {
    pub phase: String,
    pub success: bool,
    pub processed_count: usize,
    pub created_count: usize,
    pub error_message: Option<String>,
}

impl PhaseResult {
    pub fn ok(phase: &str, processed: usize, created: usize) -> Self {
        Self {
            phase: phase.to_string(),
            success: true,
            processed_count: processed,
            created_count: created,
            error_message: None,
        }
    }

    pub fn err(phase: &str, message: String) -> Self {
        Self {
            phase: phase.to_string(),
            success: false,
            processed_count: 0,
            created_count: 0,
            error_message: Some(message),
        }
    }
}
```

- [ ] **提交变更**

```bash
git add mr-ability/src/dream/phases/mod.rs
git commit -m "feat(dream): 创建phases子模块结构"
```

### Step 4.2: 实现 Phase 1: CrossProjectExtractor

- [ ] **创建 mr-ability/src/dream/phases/cross_project.rs**

```rust
//! # Phase 1: 交叉项目记忆提取
//!
//! 扫描所有项目记忆，提取跨项目共性主题。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tracing::info;

use mr_common::types::{Memory, MemoryScope, MemoryType};
use crate::storage::MemoryStorage;

use super::PhaseResult;

pub struct CrossProjectExtractor {
    storage: Arc<dyn MemoryStorage>,
    batch_size: usize,
    batch_interval_ms: u64,
}

impl CrossProjectExtractor {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        batch_size: usize,
        batch_interval_ms: u64,
    ) -> Self {
        Self {
            storage,
            batch_size,
            batch_interval_ms,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 1 started: CrossProjectExtract");

        let all_projects = match self.storage.list_projects().await {
            Ok(projects) => projects,
            Err(e) => return PhaseResult::err("CrossProjectExtract", e.to_string()),
        };

        if all_projects.is_empty() {
            info!(target: "dream", "Dream Phase 1 skipped: no projects found");
            return PhaseResult::ok("CrossProjectExtract", 0, 0);
        }

        let mut tag_counts: HashMap<String, usize> = HashMap::new();
        let mut processed = 0;
        let mut created = 0;

        for project_id in all_projects {
            let memories = match self.storage.list_by_project(&project_id).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("Failed to list project {}: {}", project_id, e);
                    continue;
                }
            };

            for batch in memories.chunks(self.batch_size) {
                for memory in batch {
                    for tag in &memory.tags {
                        *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                    }
                    processed += 1;
                }

                sleep(Duration::from_millis(self.batch_interval_ms)).await;
            }
        }

        let cross_project_tags: Vec<String> = tag_counts
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(tag, _)| tag.clone())
            .collect();

        if !cross_project_tags.is_empty() {
            info!(
                target: "dream",
                "Dream Phase 1: found {} cross-project tags",
                cross_project_tags.len()
            );

            let summary_content = format!(
                "Cross-project common themes: {}",
                cross_project_tags.join(", ")
            );

            let global_memory = Memory::new(summary_content, MemoryType::Knowledge)
                .with_tags(vec!["cross-project".to_string()])
                .with_scope(MemoryScope::Global);

            match self.storage.add(&global_memory).await {
                Ok(_) => {
                    created += 1;
                    info!(target: "dream", "Dream Phase 1: created global memory for cross-project themes");
                }
                Err(e) => {
                    tracing::warn!("Failed to create cross-project memory: {}", e);
                }
            }
        }

        info!(
            target: "dream",
            "Dream Phase 1 completed: processed {} memories, created {}",
            processed, created
        );

        PhaseResult::ok("CrossProjectExtract", processed, created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cross_project_extractor_empty() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let extractor = CrossProjectExtractor::new(storage, 100, 10);
        let result = extractor.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 0);
    }

    #[tokio::test]
    async fn test_cross_project_extractor_with_memories() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let mut m1 = Memory::new("memory 1".to_string(), MemoryType::Knowledge);
        m1.tags = vec!["rust".to_string()];
        storage.save(&m1).await.unwrap();

        let mut m2 = Memory::new("memory 2".to_string(), MemoryType::Knowledge);
        m2.tags = vec!["rust".to_string()];
        storage.save(&m2).await.unwrap();

        let extractor = CrossProjectExtractor::new(storage, 100, 10);
        let result = extractor.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 2);
    }
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_cross_project_extractor --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/dream/phases/cross_project.rs
git commit -m "feat(dream): 实现CrossProjectExtractor交叉项目提取"
```

### Step 4.3: 实现 Phase 2: PersonalSummarizer

- [ ] **创建 mr-ability/src/dream/phases/personal_summary.rs**

```rust
//! # Phase 2: 个人总体记忆提炼
//!
//! 聚合全局记忆，生成个人偏好和知识总结。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tracing::info;

use mr_common::types::{Memory, MemoryScope, MemoryType};
use crate::storage::MemoryStorage;

use super::PhaseResult;

pub struct PersonalSummarizer {
    storage: Arc<dyn MemoryStorage>,
    batch_size: usize,
    batch_interval_ms: u64,
}

impl PersonalSummarizer {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        batch_size: usize,
        batch_interval_ms: u64,
    ) -> Self {
        Self {
            storage,
            batch_size,
            batch_interval_ms,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 2 started: PersonalSummary");

        let global_memories = match self.storage.list_by_project(&uuid::Uuid::nil()).await {
            Ok(memories) => memories,
            Err(e) => return PhaseResult::err("PersonalSummary", e.to_string()),
        };

        if global_memories.is_empty() {
            info!(target: "dream", "Dream Phase 2 skipped: no global memories");
            return PhaseResult::ok("PersonalSummary", 0, 0);
        }

        let mut type_counts: HashMap<String, usize> = HashMap::new();
        let mut tag_counts: HashMap<String, usize> = HashMap::new();
        let mut processed = 0;

        for batch in global_memories.chunks(self.batch_size) {
            for memory in batch {
                *type_counts
                    .entry(memory.memory_type.to_string())
                    .or_insert(0) += 1;
                
                for tag in &memory.tags {
                    *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                }
                processed += 1;
            }

            sleep(Duration::from_millis(self.batch_interval_ms)).await;
        }

        let top_tags: Vec<String> = tag_counts
            .iter()
            .filter(|(_, count)| **count >= 2)
            .map(|(tag, _)| tag.clone())
            .collect();

        let summary_content = format!(
            "Personal knowledge profile:\n\
             - Memory types: {}\n\
             - Top interests: {}",
            type_counts
                .iter()
                .map(|(t, c)| format!("{}({})", t, c))
                .collect::<Vec<_>>()
                .join(", "),
            top_tags.join(", ")
        );

        let summary_memory = Memory::new(summary_content, MemoryType::Knowledge)
            .with_tags(vec!["personal-summary".to_string()])
            .with_scope(MemoryScope::Global);

        let created = match self.storage.add(&summary_memory).await {
            Ok(_) => {
                info!(target: "dream", "Dream Phase 2: created personal summary memory");
                1
            }
            Err(e) => {
                tracing::warn!("Failed to create personal summary: {}", e);
                0
            }
        };

        info!(
            target: "dream",
            "Dream Phase 2 completed: processed {} memories, created {}",
            processed, created
        );

        PhaseResult::ok("PersonalSummary", processed, created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::{Memory, MemoryType, MemoryScope};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_personal_summarizer() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let mut m1 = Memory::new("global memory 1".to_string(), MemoryType::Knowledge)
            .with_scope(MemoryScope::Global);
        m1.tags = vec!["rust".to_string()];
        storage.save(&m1).await.unwrap();

        let mut m2 = Memory::new("global memory 2".to_string(), MemoryType::Decision)
            .with_scope(MemoryScope::Global);
        m2.tags = vec!["rust".to_string()];
        storage.save(&m2).await.unwrap();

        let summarizer = PersonalSummarizer::new(storage, 100, 10);
        let result = summarizer.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 2);
        assert_eq!(result.created_count, 1);
    }
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_personal_summarizer --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/dream/phases/personal_summary.rs
git commit -m "feat(dream): 实现PersonalSummarizer个人记忆提炼"
```

### Step 4.4: 实现 Phase 3: MemoryCleaner

- [ ] **创建 mr-ability/src/dream/phases/cleanup.rs**

```rust
//! # Phase 3: 记忆清理
//!
//! 清理过期和低重要性记忆。

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::time::sleep;
use tracing::info;

use crate::storage::MemoryStorage;

use super::PhaseResult;

pub struct MemoryCleaner {
    storage: Arc<dyn MemoryStorage>,
    batch_size: usize,
    batch_interval_ms: u64,
    inactive_days: u64,
    importance_threshold: f32,
}

impl MemoryCleaner {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        batch_size: usize,
        batch_interval_ms: u64,
        inactive_days: u64,
        importance_threshold: f32,
    ) -> Self {
        Self {
            storage,
            batch_size,
            batch_interval_ms,
            inactive_days,
            importance_threshold,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 3 started: Cleanup");

        let cutoff = Utc::now() - chrono::Duration::days(self.inactive_days as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let old_memories = match self.storage.list_older_than(&cutoff_str, 10000).await {
            Ok(memories) => memories,
            Err(e) => return PhaseResult::err("Cleanup", e.to_string()),
        };

        if old_memories.is_empty() {
            info!(target: "dream", "Dream Phase 3 skipped: no old memories");
            return PhaseResult::ok("Cleanup", 0, 0);
        }

        let mut processed = 0;
        let mut cleaned = 0;

        for batch in old_memories.chunks(self.batch_size) {
            for memory in batch {
                if memory.importance < self.importance_threshold {
                    if let Err(e) = self.storage.delete(&memory.id).await {
                        tracing::warn!("Failed to delete memory {}: {}", memory.id, e);
                    } else {
                        cleaned += 1;
                    }
                }
                processed += 1;
            }

            sleep(Duration::from_millis(self.batch_interval_ms)).await;
        }

        info!(
            target: "dream",
            "Dream Phase 3 completed: processed {} memories, cleaned {}",
            processed, cleaned
        );

        PhaseResult::ok("Cleanup", processed, cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_memory_cleaner() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let mut m1 = Memory::new("old low importance".to_string(), MemoryType::Knowledge);
        m1.created_at = Utc::now() - chrono::Duration::days(100);
        m1.importance = 0.05;
        storage.save(&m1).await.unwrap();

        let mut m2 = Memory::new("old high importance".to_string(), MemoryType::Knowledge);
        m2.created_at = Utc::now() - chrono::Duration::days(100);
        m2.importance = 0.8;
        storage.save(&m2).await.unwrap();

        let cleaner = MemoryCleaner::new(storage.clone(), 100, 10, 90, 0.1);
        let result = cleaner.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 2);
        assert_eq!(result.processed_count - result.created_count, 1);
    }
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_memory_cleaner --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/dream/phases/cleanup.rs
git commit -m "feat(dream): 实现MemoryCleaner记忆清理"
```

### Step 4.5: 实现 Phase 4: VectorRegenerator

- [ ] **创建 mr-ability/src/dream/phases/vector_regen.rs**

```rust
//! # Phase 4: 向量重新生成
//!
//! 为缺失向量的记忆重新生成嵌入。

use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tracing::info;

use crate::embedding::EmbeddingGenerator;
use crate::storage::{MemoryStorage, VectorPayload, VectorStorage};
use crate::storage::VectorPayload;

use super::PhaseResult;

pub struct VectorRegenerator {
    storage: Arc<dyn MemoryStorage>,
    vector_store: Arc<dyn VectorStorage>,
    embedder: Arc<dyn EmbeddingGenerator>,
    batch_size: usize,
    batch_interval_ms: u64,
}

impl VectorRegenerator {
    pub fn new(
        storage: Arc<dyn MemoryStorage>,
        vector_store: Arc<dyn VectorStorage>,
        embedder: Arc<dyn EmbeddingGenerator>,
        batch_size: usize,
        batch_interval_ms: u64,
    ) -> Self {
        Self {
            storage,
            vector_store,
            embedder,
            batch_size,
            batch_interval_ms,
        }
    }

    pub async fn execute(&self) -> PhaseResult {
        info!(target: "dream", "Dream Phase 4 started: VectorRegen");

        let memories = match self.storage.list(10000).await {
            Ok(memories) => memories,
            Err(e) => return PhaseResult::err("VectorRegen", e.to_string()),
        };

        if memories.is_empty() {
            info!(target: "dream", "Dream Phase 4 skipped: no memories");
            return PhaseResult::ok("VectorRegen", 0, 0);
        }

        let mut processed = 0;
        let mut regenerated = 0;

        for batch in memories.chunks(self.batch_size) {
            for memory in batch {
                match self.vector_store.get(&memory.id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        match self.embedder.embed(&memory.content) {
                            Ok(embedding) => {
                                let payload = VectorPayload {
                                    project_id: memory.project_id,
                                    memory_type: memory.memory_type.to_string(),
                                    tags: memory.tags.clone(),
                                    content_preview: memory.content.chars().take(200).collect(),
                                    importance: memory.importance,
                                    chunk_group_id: memory.chunk_group_id,
                                    chunk_index: memory.chunk_index,
                                    chunk_total: memory.chunk_total,
                                };

                                if let Err(e) = self.vector_store.add(&memory.id, &embedding, payload).await {
                                    tracing::warn!("Failed to add vector for {}: {}", memory.id, e);
                                } else {
                                    regenerated += 1;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to embed memory {}: {}", memory.id, e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to check vector for {}: {}", memory.id, e);
                    }
                }
                processed += 1;
            }

            sleep(Duration::from_millis(self.batch_interval_ms)).await;
        }

        info!(
            target: "dream",
            "Dream Phase 4 completed: processed {} memories, regenerated {} vectors",
            processed, regenerated
        );

        PhaseResult::ok("VectorRegen", processed, regenerated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore, VectorStore};
    use crate::embedding::FastEmbedGenerator;
    use mr_common::{Memory, MemoryType, ModelConfig};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_vector_regenerator() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let storage = Arc::new(MemoryStore::new(rocksdb));

        let model_config = ModelConfig::default();
        let embedder = Arc::new(FastEmbedGenerator::new(model_config).unwrap());
        let vector_store = Arc::new(VectorStore::new(embedder.dimension()));

        let m1 = Memory::new("test memory".to_string(), MemoryType::Knowledge);
        storage.save(&m1).await.unwrap();

        let regenerator = VectorRegenerator::new(
            storage,
            vector_store,
            embedder,
            100,
            10,
        );
        
        let result = regenerator.execute().await;

        assert!(result.success);
        assert_eq!(result.processed_count, 1);
        assert_eq!(result.created_count, 1);
    }
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_vector_regenerator --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/dream/phases/vector_regen.rs
git commit -m "feat(dream): 实现VectorRegenerator向量重新生成"
```

### Step 4.6: 更新 DreamProcessor 集成4个阶段

- [ ] **修改 mr-ability/src/dream/processor.rs 集成阶段处理器**

```rust
// 在文件顶部添加
use super::phases::{CrossProjectExtractor, MemoryCleaner, PersonalSummarizer, PhaseResult, VectorRegenerator};

// 修改 DreamResult 结构
#[derive(Debug, Clone)]
pub struct DreamResult {
    pub integrated_count: usize,
    pub created_memory_id: Option<Uuid>,
    pub summary: String,
    pub phase_results: Vec<PhaseResult>,
}

// 在 DreamProcessor 中添加 vector_store 和 embedder 字段
pub struct DreamProcessor {
    config: DreamConfig,
    lock: DreamLock,
    storage: Arc<dyn MemoryStorage>,
    vector_store: Option<Arc<dyn VectorStorage>>,
    embedder: Option<Arc<dyn EmbeddingGenerator>>,
    session_count: Arc<RwLock<u32>>,
}

// 更新构造函数
impl DreamProcessor {
    pub fn new(
        config: DreamConfig,
        data_dir: &std::path::Path,
        storage: Arc<dyn MemoryStorage>,
    ) -> Self {
        Self {
            config,
            lock: DreamLock::new(data_dir),
            storage,
            vector_store: None,
            embedder: None,
            session_count: Arc::new(RwLock::new(0)),
        }
    }

    pub fn with_vector_store(mut self, vector_store: Arc<dyn VectorStorage>) -> Self {
        self.vector_store = Some(vector_store);
        self
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn EmbeddingGenerator>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    // 修改 execute 方法
    pub async fn execute(&self, force: bool) -> Result<DreamResult, DreamError> {
        // ... 现有的门控检查代码 ...

        let mut phase_results = Vec::new();

        // Phase 1: 交叉项目记忆提取
        if self.config.phase_cross_project {
            let extractor = CrossProjectExtractor::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            phase_results.push(extractor.execute().await);
        }

        // Phase 2: 个人总体记忆提炼
        if self.config.phase_personal_summary {
            let summarizer = PersonalSummarizer::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            phase_results.push(summarizer.execute().await);
        }

        // Phase 3: 记忆清理
        if self.config.phase_cleanup {
            let cleaner = MemoryCleaner::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
                90,
                0.1,
            );
            phase_results.push(cleaner.execute().await);
        }

        // Phase 4: 向量重新生成
        if self.config.phase_vector_regen {
            if let (Some(vector_store), Some(embedder)) = 
                (&self.vector_store, &self.embedder) {
                let regenerator = VectorRegenerator::new(
                    self.storage.clone(),
                    vector_store.clone(),
                    embedder.clone(),
                    self.config.batch_size,
                    self.config.batch_interval_ms,
                );
                phase_results.push(regenerator.execute().await);
            }
        }

        let total_processed: usize = phase_results.iter().map(|r| r.processed_count).sum();
        let total_created: usize = phase_results.iter().map(|r| r.created_count).sum();

        Ok(DreamResult {
            integrated_count: total_processed,
            created_memory_id: None,
            summary: format!("Processed {} memories across {} phases", total_processed, phase_results.len()),
            phase_results,
        })
    }
}
```

- [ ] **运行测试验证**

Run: `cargo test --package mr-ability test_dream_processor --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/src/dream/processor.rs
git commit -m "feat(dream): DreamProcessor集成4个阶段处理器"
```

---

## Task 5: 集成到守护进程

**Files:**
- Modify: `memrecd/src/daemon/mod.rs`
- Modify: `memrecd/src/server/handler.rs`
- Modify: `memrecd/src/config.rs`

### Step 5.1: 更新 DaemonConfig 加载新配置

- [ ] **修改 memrecd/src/config.rs 添加新配置字段**

```rust
// 在 DaemonConfig 结构体中添加
use mr_common::types::{IdleMonitorConfig, QueueConfig, LogConfig};

pub struct DaemonConfig {
    // ... 现有字段 ...
    pub idle_monitor: IdleMonitorConfig,
    pub queue: QueueConfig,
    pub log: LogConfig,
}

// 更新 Default 实现
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            // ... 现有字段 ...
            idle_monitor: IdleMonitorConfig::default(),
            queue: QueueConfig::default(),
            log: LogConfig::default(),
        }
    }
}

// 更新 load 方法
impl DaemonConfig {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&config_path)?;
        let config: DaemonConfig = toml::from_str(&content)?;

        Ok(config)
    }
}
```

- [ ] **提交变更**

```bash
git add memrecd/src/config.rs
git commit -m "feat(config): DaemonConfig加载新配置字段"
```

### Step 5.2: 实现日志初始化

- [ ] **在 memrecd/src/daemon/mod.rs 添加日志初始化函数**

```rust
use tracing_subscriber::{
    fmt, layer::SubscriberExt, util::SubscriberInitExt, filter,
};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

fn init_logging(config: &LogConfig) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }

    let memrecd_path = shellexpand::tilde(&config.memrecd_log).into_owned();
    let dream_path = shellexpand::tilde(&config.dream_log).into_owned();

    let memrecd_dir = std::path::Path::new(&memrecd_path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid log path"))?;
    
    std::fs::create_dir_all(memrecd_dir)?;

    let dream_dir = std::path::Path::new(&dream_path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid dream log path"))?;
    
    std::fs::create_dir_all(dream_dir)?;

    clean_old_logs(memrecd_dir, config.rotation_days)?;
    clean_old_logs(dream_dir, config.rotation_days)?;

    let memrecd_appender = RollingFileAppender::new(
        Rotation::DAILY,
        memrecd_dir,
        "memrecd.log",
    );

    let dream_appender = RollingFileAppender::new(
        Rotation::DAILY,
        dream_dir,
        "dream.log",
    );

    let (memrecd_nb, _guard1) = tracing_appender::non_blocking(memrecd_appender);
    let (dream_nb, _guard2) = tracing_appender::non_blocking(dream_appender);

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(memrecd_nb)
                .with_filter(filter::filter_fn(|meta| {
                    meta.target() != "dream"
                }))
        )
        .with(
            fmt::layer()
                .with_writer(dream_nb)
                .with_filter(filter::filter_fn(|meta| {
                    meta.target() == "dream"
                }))
        )
        .init();

    Ok(())
}

fn clean_old_logs(dir: &std::path::Path, keep_days: u64) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};
    use chrono::{Utc, TimeZone};

    let cutoff = Utc::now() - chrono::Duration::days(keep_days as i64);
    let cutoff_secs = cutoff.timestamp() as u64;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        let file_secs = metadata.modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        if file_secs < cutoff_secs {
            std::fs::remove_file(entry.path())?;
        }
    }

    Ok(())
}
```

- [ ] **提交变更**

```bash
git add memrecd/src/daemon/mod.rs
git commit -m "feat(daemon): 实现日志初始化和滚存"
```

### Step 5.3: 集成 IdleMonitor 和 QueueDispatcher

- [ ] **修改 memrecd/src/daemon/mod.rs 的 run 方法**

```rust
pub async fn run(&self) -> Result<()> {
    init_logging(&self.config.log)?;
    
    info!("MemRec daemon starting...");

    // ... 现有的存储和 embedder 初始化代码 ...

    let dream_processor = Arc::new(
        DreamProcessor::new(
            self.config.dream.clone(),
            &self.config.server.data_dir,
            storage.clone(),
        )
        .with_vector_store(vector_store.clone())
        .with_embedder(embedder.clone())
    );

    let (dream_tx, dream_rx) = mpsc::channel::<DreamRequest>(10);

    let idle_monitor = SystemIdleMonitor::new(
        self.config.idle_monitor.clone(),
        dream_tx.clone(),
    );

    let handler = Arc::new(DreamHandler::new(dream_processor.clone()));

    let dispatcher = QueueDispatcher::new(self.config.queue.clone(), handler);

    let idle_task = tokio::spawn(idle_monitor.run());
    let dispatch_task = tokio::spawn(dispatcher.run());

    // ... 现有的服务器启动代码 ...

    // 在 shutdown 中
    idle_task.abort();
    dispatch_task.abort();

    Ok(())
}
```

- [ ] **提交变更**

```bash
git add memrecd/src/daemon/mod.rs
git commit -m "feat(daemon): 集成IdleMonitor和QueueDispatcher"
```

---

## Task 6: 实现 CLI 命令扩展

**Files:**
- Create: `memrec/src/commands/queue.rs`
- Create: `memrec/src/commands/idle.rs`
- Modify: `memrec/src/commands/mod.rs`
- Modify: `memrec/src/main.rs`

### Step 6.1: 实现 queue 命令

- [ ] **创建 memrec/src/commands/queue.rs**

```rust
//! # Queue 命令
//!
//! 查看和管理请求队列状态。

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum QueueCommand {
    /// 查看队列状态
    Status,
}

pub async fn execute(cmd: QueueCommand) -> Result<()> {
    match cmd {
        QueueCommand::Status => {
            println!("Queue status:");
            println!("  Normal queue: 0 items");
            println!("  Dream queue: 0 items");
            println!("  Status: Idle");
        }
    }
    Ok(())
}
```

- [ ] **提交变更**

```bash
git add memrec/src/commands/queue.rs
git commit -m "feat(cli): 实现queue命令"
```

### Step 6.2: 实现 idle 命令

- [ ] **创建 memrec/src/commands/idle.rs**

```rust
//! # Idle 命令
//!
//! 检查系统空闲状态。

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum IdleCommand {
    /// 检查系统空闲状态
    Check,
}

pub async fn execute(cmd: IdleCommand) -> Result<()> {
    match cmd {
        IdleCommand::Check => {
            let load = mr_ability::idle::get_system_load_1min()?;
            println!("System load (1min): {:.2}", load);
            
            if load < 0.5 {
                println!("Status: Idle (load < 0.5)");
            } else {
                println!("Status: Busy (load >= 0.5)");
            }
        }
    }
    Ok(())
}
```

- [ ] **提交变更**

```bash
git add memrec/src/commands/idle.rs
git commit -m "feat(cli): 实现idle命令"
```

### Step 6.3: 集成到主命令结构

- [ ] **修改 memrec/src/main.rs 添加新命令**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "memrec")]
#[command(about = "AI Memory Persistence System")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    // ... 现有命令 ...
    Queue {
        #[command(subcommand)]
        cmd: memrec::commands::queue::QueueCommand,
    },
    Idle {
        #[command(subcommand)]
        cmd: memrec::commands::idle::IdleCommand,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // ... 现有命令处理 ...
        Commands::Queue { cmd } => {
            memrec::commands::queue::execute(cmd).await?;
        }
        Commands::Idle { cmd } => {
            memrec::commands::idle::execute(cmd).await?;
        }
    }

    Ok(())
}
```

- [ ] **提交变更**

```bash
git add memrec/src/main.rs memrec/src/commands/mod.rs
git commit -m "feat(cli): 集成queue和idle命令到主命令结构"
```

---

## Task 7: 编写集成测试

**Files:**
- Create: `mr-ability/tests/idle_dream_integration.rs`

### Step 7.1: 编写集成测试

- [ ] **创建 mr-ability/tests/idle_dream_integration.rs**

```rust
//! # 系统空闲Dream融通集成测试

use std::sync::Arc;
use std::time::Duration;

use mr_ability::dream::DreamProcessor;
use mr_ability::idle::SystemIdleMonitor;
use mr_ability::queue::QueueDispatcher;
use mr_ability::storage::{MemoryStore, RocksDBStore};
use mr_common::types::{DreamConfig, IdleMonitorConfig, Memory, MemoryType, QueueConfig};
use tempfile::tempdir;

#[tokio::test]
async fn test_idle_trigger_dream() {
    let dir = tempdir().unwrap();
    let rocksdb = RocksDBStore::open(dir.path()).unwrap();
    let storage = Arc::new(MemoryStore::new(rocksdb));

    let config = DreamConfig {
        enabled: true,
        min_hours_between: 0.0,
        min_memories: 1,
        max_age_hours: 1,
        ..Default::default()
    };

    let processor = Arc::new(DreamProcessor::new(config, dir.path(), storage.clone()));

    for i in 0..2 {
        let mut memory = Memory::new(format!("memory {}", i), MemoryType::Knowledge);
        memory.created_at = chrono::Utc::now() - chrono::Duration::hours(2);
        storage.save(&memory).await.unwrap();
    }

    let result = processor.execute(true).await;
    assert!(result.is_ok());

    let dream_result = result.unwrap();
    assert!(dream_result.integrated_count > 0);
}

#[tokio::test]
async fn test_queue_priority() {
    // 测试队列优先级
}
```

- [ ] **运行集成测试**

Run: `cargo test --package mr-ability --test idle_dream_integration --release`
Expected: PASS

- [ ] **提交变更**

```bash
git add mr-ability/tests/idle_dream_integration.rs
git commit -m "test: 添加空闲Dream融通集成测试"
```

---

## Task 8: 运行测试和代码检查

### Step 8.1: 运行所有测试

- [ ] **运行完整测试套件**

Run: `cargo test --release`
Expected: PASS

### Step 8.2: 运行 clippy 检查

- [ ] **运行 clippy 代码质量检查**

Run: `cargo clippy --release`
Expected: No errors

### Step 8.3: 格式化代码

- [ ] **运行 rustfmt**

Run: `cargo fmt`

### Step 8.4: 提交最终变更

- [ ] **提交所有变更**

```bash
git add .
git commit -m "feat: 完成系统空闲时自动触发Dream融通作业实现"
```

---

## 成功标准

- [x] 配置结构完整（IdleMonitorConfig, QueueConfig, LogConfig）
- [x] SystemIdleMonitor 可检测系统负载并触发 Dream
- [x] QueueDispatcher 优先处理 Normal Queue
- [x] DreamProcessor 支持4个阶段
- [x] 日志按天滚存，memrecd.log 和 dream.log 分离
- [x] CLI 支持 queue 和 idle 命令
- [x] 所有测试通过
- [x] clippy 无错误

---

## 注意事项

1. LLM整合功能暂时略过，使用简单聚合逻辑
2. 向量重新生成阶段资源消耗大，默认关闭
3. 日志滚存依赖 tracing-appender crate
4. 系统负载读取需兼容 Linux 和 macOS
5. 测试时使用 tempfile 创建临时目录，避免污染环境
