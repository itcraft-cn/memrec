# MemRec Phase 4: CLI 工具实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 memrec CLI 工具，提供命令行接口与 memrecd 服务通信。

**Architecture:** clap 命令解析 + Unix Socket 客户端 + JSON-RPC 协议 + 结果格式化。

**Tech Stack:** Rust, clap, tokio, serde_json

---

## 前置条件

Phase 1-3 已完成：
- common crate 中的协议类型
- memrecd 服务已可运行

---

## 文件结构

### 新建文件

```
memrec/
├── Cargo.toml               # 添加 clap 依赖
└── src/
    ├── client/
    │   ├── mod.rs           # client 模块导出
    │   ├── connection.rs    # Unix Socket 客户端
    ├── commands/
    │   ├── mod.rs           # commands 模块导出
    │   ├── memory.rs        # 记忆命令
    │   ├── search.rs        # 检索命令
    │   ├── project.rs       # 项目命令
    │   ├── config.rs        # 配置命令
    │   ├── daemon.rs        # 守护进程命令
    │   └── stats.rs         # 统计命令
    ├── formatter/
    │   ├── mod.rs           # formatter 模块导出
    │   ├── output.rs        # 输出格式化
    ├── main.rs              # CLI 入口
```

---

## Task 1: 添加 CLI 依赖

**Files:**
- Modify: `memrec/Cargo.toml`

- [ ] **Step 1: 更新 memrec/Cargo.toml**

```toml
[package]
name = "memrec"
version.workspace = true
edition.workspace = true

[[bin]]
name = "memrec"
path = "src/main.rs"

[dependencies]
memrec-common = { path = "../common" }
anyhow.workspace = true
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive", "color"] }
dirs = "5"
colored = "2"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: 验证依赖**

```bash
cargo check -p memrec
```

Expected: PASS

- [ ] **Step 3: 提交依赖配置**

```bash
git add memrec/Cargo.toml
git commit -m "feat: add CLI dependencies"
```

---

## Task 2: 实现 Unix Socket 客户端

**Files:**
- Create: `memrec/src/client/mod.rs`
- Create: `memrec/src/client/connection.rs`

- [ ] **Step 1: 创建 client 目录**

```bash
mkdir -p memrec/src/client
```

- [ ] **Step 2: 实现 Unix Socket 客户端**

File: `memrec/src/client/connection.rs`

```rust
use anyhow::{Result, Context};
use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::path::PathBuf;
use memrec_common::{JsonRpcRequest, JsonRpcResponse};

pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    pub fn new() -> Result<Self> {
        let socket_path = Self::default_socket_path()?;
        Ok(Self { socket_path })
    }
    
    pub fn with_socket_path(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }
    
    fn default_socket_path() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .context("Failed to get home directory")?;
        Ok(home.join(".memrec").join("memrecd.sock"))
    }
    
    pub async fn send(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to memrecd")?;
        
        let request_json = serde_json::to_string(request)
            .context("Failed to serialize request")?;
        
        stream.write_all(request_json.as_bytes())
            .await
            .context("Failed to send request")?;
        
        stream.flush()
            .await
            .context("Failed to flush stream")?;
        
        let mut buffer = vec![0u8; 8192];
        let n = stream.read(&mut buffer)
            .await
            .context("Failed to read response")?;
        
        let response_json = String::from_utf8_lossy(&buffer[..n]);
        let response: JsonRpcResponse = serde_json::from_str(&response_json)
            .context("Failed to parse response")?;
        
        Ok(response)
    }
    
    pub async fn check_connection(&self) -> Result<bool> {
        if !self.socket_path.exists() {
            return Ok(false);
        }
        
        UnixStream::connect(&self.socket_path)
            .await
            .map(|_| true)
            .or_else(|_| Ok(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_client_default_socket_path() {
        let path = Client::default_socket_path().unwrap();
        assert!(path.to_string_lossy().contains(".memrec"));
    }
    
    #[test]
    fn test_client_with_custom_socket_path() {
        let custom_path = PathBuf::from("/tmp/test.sock");
        let client = Client::with_socket_path(custom_path.clone());
        assert_eq!(client.socket_path, custom_path);
    }
}
```

- [ ] **Step 3: 创建 client/mod.rs**

File: `memrec/src/client/mod.rs`

```rust
mod connection;

pub use connection::Client;
```

- [ ] **Step 4: 运行客户端测试**

```bash
cargo test -p memrec --lib client::connection::tests
```

Expected: 2 tests PASS

- [ ] **Step 5: 提交客户端实现**

```bash
git add memrec/src/client/
git commit -m "feat: implement Unix socket client"
```

---

## Task 3: 实现输出格式化

**Files:**
- Create: `memrec/src/formatter/mod.rs`
- Create: `memrec/src/formatter/output.rs`

- [ ] **Step 1: 创建 formatter 目录**

```bash
mkdir -p memrec/src/formatter
```

- [ ] **Step 2: 实现输出格式化器**

File: `memrec/src/formatter/output.rs`

```rust
use colored::Colorize;
use memrec_common::{Memory, Project, MemoryType};

pub enum OutputFormat {
    Default,
    Json,
}

pub struct Formatter {
    format: OutputFormat,
}

impl Formatter {
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }
    
    pub fn format_memory(&self, memory: &Memory) -> String {
        match self.format {
            OutputFormat::Json => {
                serde_json::to_string_pretty(memory).unwrap()
            }
            OutputFormat::Default => {
                let type_str = format_memory_type(memory.memory_type);
                let time_str = memory.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
                let id_str = memory.id.to_string().chars().take(8).collect::<String>();
                
                let content_preview = if memory.content.len() > 50 {
                    format!("{}...", &memory.content[..50])
                } else {
                    memory.content.clone()
                };
                
                let mut output = format!(
                    "[{}] [{}] #{id} {content}",
                    time_str,
                    type_str,
                    id = id_str,
                    content = content_preview
                );
                
                if !memory.tags.is_empty() {
                    output.push_str(&format!("\n  Tags: {}", memory.tags.join(", ")));
                }
                
                output.push_str(&format!("\n  Importance: {:.2}", memory.importance));
                
                if memory.is_deleted {
                    output.push_str(&format!("\n  {}", "DELETED".red()));
                }
                
                output
            }
        }
    }
    
    pub fn format_memories(&self, memories: &[Memory]) -> String {
        match self.format {
            OutputFormat::Json => {
                serde_json::to_string_pretty(memories).unwrap()
            }
            OutputFormat::Default => {
                memories.iter()
                    .map(|m| self.format_memory(m))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        }
    }
    
    pub fn format_project(&self, project: &Project) -> String {
        match self.format {
            OutputFormat::Json => {
                serde_json::to_string_pretty(project).unwrap()
            }
            OutputFormat::Default => {
                let id_str = project.id.to_string().chars().take(8).collect::<String>();
                let time_str = project.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
                
                let mut output = format!(
                    "[{}] #{id} {name}",
                    time_str,
                    id = id_str,
                    name = project.name
                );
                
                if let Some(desc) = &project.description {
                    output.push_str(&format!("\n  Description: {}", desc));
                }
                
                if project.config.active {
                    output.push_str(&format!("\n  {}", "ACTIVE".green()));
                }
                
                output
            }
        }
    }
    
    pub fn format_projects(&self, projects: &[Project]) -> String {
        projects.iter()
            .map(|p| self.format_project(p))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    
    pub fn format_success(&self, message: &str) -> String {
        match self.format {
            OutputFormat::Json => {
                format!("{{\"success\": \"{}\"}}", message)
            }
            OutputFormat::Default => {
                format!("{} {}", "✓".green(), message)
            }
        }
    }
    
    pub fn format_error(&self, message: &str) -> String {
        match self.format {
            OutputFormat::Json => {
                format!("{{\"error\": \"{}\"}}", message)
            }
            OutputFormat::Default => {
                format!("{} {}", "✗".red(), message)
            }
        }
    }
}

fn format_memory_type(t: MemoryType) -> String {
    match t {
        MemoryType::Conversation => "conversation".cyan().to_string(),
        MemoryType::Knowledge => "knowledge".yellow().to_string(),
        MemoryType::Decision => "decision".magenta().to_string(),
        MemoryType::Preference => "preference".blue().to_string(),
        MemoryType::Context => "context".white().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    
    #[test]
    fn test_format_memory_default() {
        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge);
        let formatter = Formatter::new(OutputFormat::Default);
        
        let output = formatter.format_memory(&memory);
        assert!(output.contains("test content"));
        assert!(output.contains("knowledge"));
    }
    
    #[test]
    fn test_format_memory_json() {
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        let formatter = Formatter::new(OutputFormat::Json);
        
        let output = formatter.format_memory(&memory);
        assert!(output.contains("\"content\": \"test\""));
    }
    
    #[test]
    fn test_format_success() {
        let formatter = Formatter::new(OutputFormat::Default);
        let output = formatter.format_success("Operation completed");
        assert!(output.contains("Operation completed"));
    }
}
```

- [ ] **Step 3: 创建 formatter/mod.rs**

```rust
mod output;

pub use output::{Formatter, OutputFormat};
```

- [ ] **Step 4: 运行格式化器测试**

```bash
cargo test -p memrec --lib formatter::output::tests
```

Expected: 3 tests PASS

- [ ] **Step 5: 提交格式化器**

```bash
git add memrec/src/formatter/
git commit -m "feat: implement output formatter"
```

---

## Task 4: 实现命令模块

**Files:**
- Create: `memrec/src/commands/mod.rs`
- Create: `memrec/src/commands/memory.rs`
- Create: `memrec/src/commands/daemon.rs`

- [ ] **Step 1: 创建 commands 目录**

```bash
mkdir -p memrec/src/commands
```

- [ ] **Step 2: 实现记忆命令**

File: `memrec/src/commands/memory.rs`

```rust
use anyhow::Result;
use clap::Subcommand;
use memrec_common::{
    JsonRpcRequest, RequestAction, RequestParams,
    AddParams, GetParams, DeleteParams, ListParams, TagParams,
    MemoryType, ResponseResult, MemoryResult, MemoryListResult,
};

use crate::client::Client;
use crate::formatter::{Formatter, OutputFormat};

#[derive(Subcommand)]
pub enum MemoryCommands {
    Add {
        content: String,
        #[arg(short, long, default_value = "conversation")]
        type: String,
        #[arg(short, long)]
        tag: Vec<String>,
        #[arg(short, long)]
        project: Option<String>,
    },
    Get {
        id: String,
    },
    List {
        #[arg(short, long)]
        tag: Option<String>,
        #[arg(short, long)]
        type: Option<String>,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    Delete {
        id: String,
        #[arg(short, long)]
        force: bool,
    },
    Tag {
        id: String,
        tag: String,
    },
}

impl MemoryCommands {
    pub async fn execute(&self, client: &Client, format: OutputFormat) -> Result<()> {
        let formatter = Formatter::new(format);
        
        match self {
            MemoryCommands::Add { content, type, tag, project } => {
                let memory_type = parse_memory_type(type)?;
                
                let request = JsonRpcRequest::new(
                    RequestAction::Add,
                    Some(RequestParams::Add(AddParams {
                        content: content.clone(),
                        memory_type,
                        tags: tag.clone(),
                        project_id: None,
                    })),
                    1,
                );
                
                let response = client.send(&request).await?;
                
                match response.result {
                    Some(ResponseResult::Memory(MemoryResult { memory })) => {
                        println!("{}", formatter.format_memory(&memory));
                    }
                    Some(ResponseResult::Success(s)) => {
                        println!("{}", formatter.format_success(&s.message));
                    }
                    None => {
                        if let Some(err) = response.error {
                            println!("{}", formatter.format_error(&err.message));
                        }
                    }
                    _ => {}
                }
            }
            
            MemoryCommands::Get { id } => {
                let uuid = uuid::Uuid::parse_str(id)?;
                
                let request = JsonRpcRequest::new(
                    RequestAction::Get,
                    Some(RequestParams::Get(GetParams { id: uuid })),
                    1,
                );
                
                let response = client.send(&request).await?;
                
                match response.result {
                    Some(ResponseResult::Memory(MemoryResult { memory })) => {
                        println!("{}", formatter.format_memory(&memory));
                    }
                    None => {
                        if let Some(err) = response.error {
                            println!("{}", formatter.format_error(&err.message));
                        }
                    }
                    _ => {}
                }
            }
            
            MemoryCommands::List { tag, type, limit } => {
                let request = JsonRpcRequest::new(
                    RequestAction::List,
                    Some(RequestParams::List(ListParams {
                        tags: if let Some(t) = tag { vec![t.clone()] } else { vec![] },
                        memory_type: if let Some(t) = type { Some(parse_memory_type(t)?) } else { None },
                        limit: *limit,
                    })),
                    1,
                );
                
                let response = client.send(&request).await?;
                
                match response.result {
                    Some(ResponseResult::MemoryList(MemoryListResult { memories, total })) => {
                        println!("{}", formatter.format_memories(&memories));
                        println!("\nTotal: {} memories", total);
                    }
                    None => {
                        if let Some(err) = response.error {
                            println!("{}", formatter.format_error(&err.message));
                        }
                    }
                    _ => {}
                }
            }
            
            MemoryCommands::Delete { id, force } => {
                let uuid = uuid::Uuid::parse_str(id)?;
                
                let request = JsonRpcRequest::new(
                    RequestAction::Delete,
                    Some(RequestParams::Delete(DeleteParams {
                        id: uuid,
                        force: *force,
                    })),
                    1,
                );
                
                let response = client.send(&request).await?;
                
                match response.result {
                    Some(ResponseResult::Success(s)) => {
                        println!("{}", formatter.format_success(&s.message));
                    }
                    None => {
                        if let Some(err) = response.error {
                            println!("{}", formatter.format_error(&err.message));
                        }
                    }
                    _ => {}
                }
            }
            
            MemoryCommands::Tag { id, tag } => {
                let uuid = uuid::Uuid::parse_str(id)?;
                
                let request = JsonRpcRequest::new(
                    RequestAction::Tag,
                    Some(RequestParams::Tag(TagParams {
                        id: uuid,
                        tag: tag.clone(),
                    })),
                    1,
                );
                
                let response = client.send(&request).await?;
                
                match response.result {
                    Some(ResponseResult::Success(s)) => {
                        println!("{}", formatter.format_success(&s.message));
                    }
                    None => {
                        if let Some(err) = response.error {
                            println!("{}", formatter.format_error(&err.message));
                        }
                    }
                    _ => {}
                }
            }
        }
        
        Ok(())
    }
}

fn parse_memory_type(s: &str) -> Result<MemoryType> {
    match s.to_lowercase().as_str() {
        "conversation" => Ok(MemoryType::Conversation),
        "knowledge" => Ok(MemoryType::Knowledge),
        "decision" => Ok(MemoryType::Decision),
        "preference" => Ok(MemoryType::Preference),
        "context" => Ok(MemoryType::Context),
        _ => Err(anyhow::anyhow!("Invalid memory type: {}", s)),
    }
}
```

- [ ] **Step 3: 实现守护进程命令**

File: `memrec/src/commands/daemon.rs`

```rust
use anyhow::{Result, Context};
use clap::Subcommand;
use std::process::{Command, Stdio};
use colored::Colorize;

#[derive(Subcommand)]
pub enum DaemonCommands {
    Start {
        #[arg(short, long)]
        foreground: bool,
    },
    Stop,
    Status,
}

impl DaemonCommands {
    pub async fn execute(&self) -> Result<()> {
        match self {
            DaemonCommands::Start { foreground } => {
                if *foreground {
                    println!("{}", "Starting memrecd in foreground...".yellow());
                    let mut child = Command::new("memrecd")
                        .spawn()
                        .context("Failed to start memrecd")?;
                    
                    let status = child.wait()?;
                    println!("memrecd exited with status: {}", status);
                } else {
                    println!("{}", "Starting memrecd in background...".yellow());
                    let mut child = Command::new("memrecd")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .context("Failed to start memrecd")?;
                    
                    println!("{} PID: {}", "✓".green(), child.id());
                }
            }
            
            DaemonCommands::Stop => {
                println!("{}", "Stopping memrecd...".yellow());
                
                let output = Command::new("pkill")
                    .arg("memrecd")
                    .output();
                
                match output {
                    Ok(_) => {
                        println!("{}", "✓ memrecd stopped".green());
                    }
                    Err(_) => {
                        println!("{}", "✗ memrecd not running".red());
                    }
                }
            }
            
            DaemonCommands::Status => {
                let output = Command::new("pgrep")
                    .arg("memrecd")
                    .output();
                
                match output {
                    Ok(out) if out.status.success() => {
                        let pids = String::from_utf8_lossy(&out.stdout);
                        println!("{} memrecd is running (PIDs: {})", "✓".green(), pids.trim());
                    }
                    _ => {
                        println!("{}", "✗ memrecd is not running".red());
                    }
                }
            }
        }
        
        Ok(())
    }
}
```

- [ ] **Step 4: 创建 commands/mod.rs**

```rust
mod memory;
mod daemon;

pub use memory::MemoryCommands;
pub use daemon::DaemonCommands;
```

- [ ] **Step 5: 提交命令实现**

```bash
git add memrec/src/commands/
git commit -m "feat: implement CLI commands"
```

---

## Task 5: 实现 CLI 入口

**Files:**
- Modify: `memrec/src/main.rs`

- [ ] **Step 1: 实现完整 CLI 入口**

File: `memrec/src/main.rs`

```rust
mod client;
mod formatter;
mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

use client::Client;
use formatter::OutputFormat;
use commands::{MemoryCommands, DaemonCommands};

#[derive(Parser)]
#[command(name = "memrec")]
#[command(about = "Memory persistence CLI for AI tools", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    #[arg(short, long, global = true)]
    json: bool,
    
    #[arg(short, long, global = true)]
    socket: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    Memory(MemoryCommands),
    
    #[command(subcommand)]
    Daemon(DaemonCommands),
    
    Stats,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Default
    };
    
    match cli.command {
        Commands::Memory(cmd) => {
            let client = Client::new()?;
            cmd.execute(&client, format).await?;
        }
        
        Commands::Daemon(cmd) => {
            cmd.execute().await?;
        }
        
        Commands::Stats => {
            let client = Client::new()?;
            show_stats(&client, format).await?;
        }
    }
    
    Ok(())
}

async fn show_stats(client: &Client, format: OutputFormat) -> Result<()> {
    use memrec_common::{JsonRpcRequest, RequestAction, ResponseResult};
    
    let request = JsonRpcRequest::new(RequestAction::Stats, None, 1);
    let response = client.send(&request).await?;
    
    match response.result {
        Some(ResponseResult::Stats(stats)) => {
            match format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&stats)?);
                }
                OutputFormat::Default => {
                    println!("Memory Statistics:");
                    println!("  Total memories: {}", stats.total_memories);
                    println!("  Active memories: {}", stats.active_memories);
                    println!("  Deleted memories: {}", stats.deleted_memories);
                    println!("  Storage usage: {:.1}%", stats.storage_usage * 100);
                    println!("  Average importance: {:.2}", stats.avg_importance);
                }
            }
        }
        None => {
            if let Some(err) = response.error {
                println!("Error: {}", err.message);
            }
        }
        _ => {}
    }
    
    Ok(())
}
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p memrec
```

Expected: PASS

- [ ] **Step 3: 提交 CLI 入口**

```bash
git add memrec/src/main.rs
git commit -m "feat: implement CLI entry point"
```

---

## Task 6: 手动测试 CLI

- [ ] **Step 1: 启动守护进程**

```bash
cargo run -p memrecd &
```

Expected: memrecd 后台运行

- [ ] **Step 2: 测试 CLI 添加记忆**

```bash
cargo run -p memrec -- memory add "Test memory content" --type knowledge --tag test
```

Expected: 显示添加的记忆

- [ ] **Step 3: 测试 CLI 列出记忆**

```bash
cargo run -p memrec -- memory list --limit 10
```

Expected: 显示记忆列表

- [ ] **Step 4: 测试 JSON 输出**

```bash
cargo run -p memrec -- --json memory list
```

Expected: JSON 格式输出

- [ ] **Step 5: 测试守护进程状态**

```bash
cargo run -p memrec -- daemon status
```

Expected: 显示运行状态

- [ ] **Step 6: 停止守护进程**

```bash
cargo run -p memrec -- daemon stop
```

Expected: 停止成功

---

## Task 7: 最终验证

- [ ] **Step 1: 运行所有测试**

```bash
cargo test --workspace
```

Expected: 所有测试 PASS

- [ ] **Step 2: 检查代码质量**

```bash
cargo clippy --workspace
```

Expected: 无严重警告

- [ ] **Step 3: Phase 4 完成提交**

```bash
git log --oneline -10
```

---

## Phase 4 完成检查清单

- [x] Unix Socket 客户端
- [x] 输出格式化器（Default/JSON）
- [x] Memory 命令
- [x] Daemon 命令
- [x] Stats 命令
- [x] CLI 入口点（clap）
- [x] 手动测试通过

**下一阶段:** Phase 5 - 高级功能（生命周期管理、压缩、遗忘）