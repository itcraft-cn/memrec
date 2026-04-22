mod client;
mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

use client::Client;
use commands::{add, get, list, delete, stats};

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
    },
    Get {
        id: String,
    },
    List {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    Delete {
        id: String,
    },
    Stats,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    let client = Client::new()?;
    
    match cli.command {
        Commands::Add { content, mtype, tag } => {
            add(&client, content, mtype, tag).await?;
        }
        Commands::Get { id } => {
            get(&client, id).await?;
        }
        Commands::List { limit } => {
            list(&client, limit).await?;
        }
        Commands::Delete { id } => {
            delete(&client, id).await?;
        }
        Commands::Stats => {
            stats(&client).await?;
        }
    }
    
    Ok(())
}