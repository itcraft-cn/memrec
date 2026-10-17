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
use uuid::Uuid;

use client::Client;
use commands::{
    add, delete, dream, dream_report, get, list, restore, scene, search_execute, stats, version,
    AddOptions,
    ListOptions, SearchArgs,
};
use commands::{detect_working_dir, read_project_uuid, resolve_scope, Scope};

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
        /// 跳过写入去重检测
        #[arg(long)]
        no_dedup: bool,
    },
    Get {
        id: String,
        #[arg(long)]
        merge: bool,
    },
    List {
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// 跳过前 N 条（与 limit 配合分页）
        #[arg(long, default_value_t = 0)]
        skip: usize,
        /// 仅当前项目（未指定时按 .mr_pid 是否存在推断）
        #[arg(short = 'p', long, group = "scope")]
        project: bool,
        /// 仅公共记忆（未指定时按 .mr_pid 是否存在推断）
        #[arg(short = 'g', long, group = "scope")]
        global: bool,
        /// 跨所有项目
        #[arg(short = 'a', long, group = "scope")]
        all: bool,
        /// 按标签过滤（可多个，全部命中）
        #[arg(long)]
        tag: Vec<String>,
        /// 按记忆类型过滤（decision/knowledge/context/preference/conversation）
        #[arg(long)]
        type_filter: Option<String>,
        /// 包含已删除（软删除）的原始记录
        #[arg(long)]
        deleted: bool,
    },
    Delete {
        id: String,
    },
    /// 恢复软删除记忆（重新加入检索索引）
    Restore {
        id: String,
    },
    Dream {
        #[arg(long)]
        force: bool,
        /// 预演：只计算不写库、不删数据、不建边
        #[arg(long)]
        dry_run: bool,
    },
    /// 查看最近若干次 Dream 运行审计报告
    DreamReport {
        /// 返回最近条数（默认 10）
        #[arg(long, default_value_t = 10)]
        last: usize,
    },
    Stats,
    Search(SearchArgs),
    Version,
    #[command(subcommand)]
    Scene(SceneCommands),
}

#[derive(Subcommand)]
enum SceneCommands {
    Create {
        theme: String,
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
    },
    Get {
        id: String,
    },
    List {
        #[arg(short, long, default_value = "20")]
        limit: usize,
        #[arg(long)]
        sort_by_heat: bool,
        #[arg(long)]
        project_id: Option<String>,
    },
    Delete {
        id: String,
    },
    AddMemory {
        scene_id: String,
        memory_id: String,
    },
    RemoveMemory {
        scene_id: String,
        memory_id: String,
    },
    UpdateHeat {
        scene_id: String,
        heat: f64,
    },
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
            no_dedup,
        } => {
            let working_dir = if global {
                None
            } else {
                Some(detect_working_dir()?)
            };
            add(
                &client,
                AddOptions {
                    content,
                    mtype,
                    tags: tag,
                    is_global: global,
                    working_dir,
                    source,
                    scope,
                    dedup: Some(!no_dedup),
                },
                human,
            )
            .await?;
        }
        Commands::Get { id, merge } => {
            get(&client, id, merge, human).await?;
        }
        Commands::List {
            limit,
            skip,
            project,
            global,
            all,
            tag,
            type_filter,
            deleted,
        } => {
            // 显式三态参数优先，未指定时按 .mr_pid 是否存在推断默认范围
            let scope = resolve_scope(project, global, all);
            let (project_only, global_only, project_id) = match scope {
                Scope::Project => {
                    // 解析 .mr_pid 中的项目 UUID，将 list 限定到当前项目
                    let root = std::path::PathBuf::from(detect_working_dir()?);
                    (true, false, read_project_uuid(&root))
                }
                Scope::Global => (false, true, None),
                Scope::All => (false, false, None),
            };
            list(
                &client,
                ListOptions {
                    limit,
                    skip,
                    project_only,
                    global_only,
                    project_id,
                    tags: tag,
                    memory_type: type_filter,
                    include_deleted: deleted,
                },
                human,
            )
            .await?;
        }
        Commands::Delete { id } => {
            delete(&client, id, human).await?;
        }
        Commands::Restore { id } => {
            restore(&client, id, human).await?;
        }
        Commands::Dream { force, dry_run } => {
            dream(&client, force, dry_run, human).await?;
        }
        Commands::DreamReport { last } => {
            dream_report(&client, last, human).await?;
        }
        Commands::Stats => {
            stats(&client, human).await?;
        }
        Commands::Search(args) => {
            // 显式三态参数优先，未指定时按 .mr_pid 是否存在推断默认范围
            let scope = resolve_scope(args.project, args.global, args.all);
            let working_dir = if scope == Scope::Project {
                Some(detect_working_dir()?)
            } else {
                None
            };
            search_execute(&client, args, scope, working_dir, human).await?;
        }
        Commands::Version => {
            version(&client, human).await?;
        }
        Commands::Scene(cmd) => match cmd {
            SceneCommands::Create {
                theme,
                project_id,
                tag,
            } => {
                let project_uuid = project_id.and_then(|s| Uuid::parse_str(&s).ok());
                scene::create(&client, theme, project_uuid, tag, human).await?;
            }
            SceneCommands::Get { id } => {
                let uuid = Uuid::parse_str(&id)?;
                scene::get(&client, uuid, human).await?;
            }
            SceneCommands::List {
                limit,
                sort_by_heat,
                project_id,
            } => {
                let project_uuid = project_id.and_then(|s| Uuid::parse_str(&s).ok());
                scene::list(&client, limit, sort_by_heat, project_uuid, human).await?;
            }
            SceneCommands::Delete { id } => {
                let uuid = Uuid::parse_str(&id)?;
                scene::delete(&client, uuid, human).await?;
            }
            SceneCommands::AddMemory {
                scene_id,
                memory_id,
            } => {
                let scene_uuid = Uuid::parse_str(&scene_id)?;
                let memory_uuid = Uuid::parse_str(&memory_id)?;
                scene::add_memory(&client, scene_uuid, memory_uuid, human).await?;
            }
            SceneCommands::RemoveMemory {
                scene_id,
                memory_id,
            } => {
                let scene_uuid = Uuid::parse_str(&scene_id)?;
                let memory_uuid = Uuid::parse_str(&memory_id)?;
                scene::remove_memory(&client, scene_uuid, memory_uuid, human).await?;
            }
            SceneCommands::UpdateHeat { scene_id, heat } => {
                let uuid = Uuid::parse_str(&scene_id)?;
                scene::update_heat(&client, uuid, heat as f32, human).await?;
            }
        },
    }

    Ok(())
}
