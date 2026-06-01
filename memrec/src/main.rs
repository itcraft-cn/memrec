mod client;
mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

use client::Client;
use commands::{add, get, list, delete, stats, search_execute, version, SearchArgs};

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
    #[command(subcommand)]
    command: Commands,
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
    
    let client = Client::new()?;
    
    match cli.command {
        Commands::Add { content, mtype, tag, global } => {
            let working_dir = if global {
                None
            } else {
                Some(detect_working_dir()?)
            };
            add(&client, content, mtype, tag, global, working_dir).await?;
        }
        Commands::Get { id, merge } => {
            get(&client, id, merge).await?;
        }
        Commands::List { limit, project_only, global_only } => {
            list(&client, limit, project_only, global_only).await?;
        }
        Commands::Delete { id } => {
            delete(&client, id).await?;
        }
        Commands::Stats => {
            stats(&client).await?;
        }
        Commands::Search(args) => {
            let working_dir = if args.all || args.global_only {
                None
            } else {
                Some(detect_working_dir()?)
            };
            search_execute(&client, args, working_dir).await?;
        }
        Commands::Version => {
            version(&client).await?;
        }
    }
    
    Ok(())
}