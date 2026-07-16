//! # memrec — MemRec CLI 工具
//!
//! 命令行客户端，通过 Unix Socket 与 memrecd 守护进程通信。
//!
//! ## 子命令
//!
//! - `add`：添加记忆（支持自动分块）
//! - `get`：获取记忆（支持分块合并）
//! - `list`：列出记忆
//! - `delete`：删除记忆
//! - `search`：语义搜索
//! - `stats`：统计信息
//! - `version`：版本号
//!
//! ## MCP 模式
//!
//! 使用 `--mcp` 启动 MCP（Model Context Protocol）服务器，
//! 通过 stdin/stdout 与 AI 工具通信。

mod client;
mod commands;
mod mcp;

use anyhow::Result;
use clap::{Parser, Subcommand};

use client::Client;
use commands::{add, delete, get, list, search_execute, stats, version, SearchArgs};

/// 检测当前工作目录，优先使用 Git 仓库根目录。
fn detect_working_dir() -> Result<String> {
    let current = std::env::current_dir()?;

    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&current)
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    Ok(current.to_string_lossy().to_string())
}

#[derive(Parser)]
#[command(name = "memrec")]
#[command(about = "Memory persistence CLI for AI tools", long_about = None)]
#[command(version)]
struct Cli {
    #[arg(long)]
    mcp: bool,

    /// 人类可读输出（默认输出 JSON-RPC）
    #[arg(long, global = true)]
    human: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Add {
        content: String,
        #[arg(short = 't', long)]
        mtype: String,
        #[arg(short, long)]
        tag: Vec<String>,
        #[arg(long)]
        global: bool,
        #[arg(short = 'S', long)]
        source: Option<String>,
        #[arg(short = 'c', long)]
        scope: Option<String>,
    },
    Get {
        id: String,
        #[arg(long)]
        merge: bool,
    },
    List {
        #[arg(short, long, default_value = "20")]
        limit: usize,
        #[arg(long)]
        project_only: bool,
        #[arg(long)]
        global_only: bool,
    },
    Delete {
        id: String,
    },
    Stats,
    Search(SearchArgs),
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.mcp {
        let server = mcp::McpServer::new();
        return server.run().await;
    }

    let command = cli.command.unwrap_or(Commands::Stats);
    let client = Client::new()?;
    let human = cli.human;

    match command {
        Commands::Add {
            content,
            mtype,
            tag,
            global,
            source,
            scope,
        } => {
            let working_dir = if global {
                None
            } else {
                Some(detect_working_dir()?)
            };
            add(&client, content, mtype, tag, global, working_dir, source, scope, human).await?;
        }
        Commands::Get { id, merge } => {
            get(&client, id, merge, human).await?;
        }
        Commands::List {
            limit,
            project_only,
            global_only,
        } => {
            list(&client, limit, project_only, global_only, human).await?;
        }
        Commands::Delete { id } => {
            delete(&client, id, human).await?;
        }
        Commands::Stats => {
            stats(&client, human).await?;
        }
        Commands::Search(args) => {
            let working_dir = if args.all || args.global_only {
                None
            } else {
                Some(detect_working_dir()?)
            };
            search_execute(&client, args, working_dir, human).await?;
        }
        Commands::Version => {
            version(&client, human).await?;
        }
    }

    Ok(())
}
