use clap::Args;
use uuid::Uuid;
use memrec_common::{
    JsonRpcRequest, RequestAction, RequestParams,
    SearchMemoryParams, MemoryType, default_min_score,
};
use crate::client::Client;

#[derive(Args, Debug)]
pub struct SearchArgs {
    #[arg(required = true)]
    query: String,
    
    #[arg(short = 'k', long, default_value = "10")]
    top_k: usize,
    
    #[arg(long, default_value_t = default_min_score())]
    min_score: f32,
    
    #[arg(long)]
    project_only: bool,
    
    #[arg(long)]
    global_only: bool,
    
    #[arg(long)]
    mtype: Option<String>,
    
    #[arg(long)]
    human: bool,
}

pub async fn execute(client: &Client, args: SearchArgs) -> anyhow::Result<()> {
    let memory_type = args.mtype.and_then(|t| match t.to_lowercase().as_str() {
        "decision" => Some(MemoryType::Decision),
        "knowledge" => Some(MemoryType::Knowledge),
        "context" => Some(MemoryType::Context),
        "preference" => Some(MemoryType::Preference),
        "conversation" => Some(MemoryType::Conversation),
        _ => None,
    });
    
    let project_id = if args.global_only {
        Some(Uuid::nil())
    } else {
        None
    };
    
    let request = JsonRpcRequest::new(
        RequestAction::SearchMemory,
        Some(RequestParams::SearchMemory(SearchMemoryParams {
            query: args.query,
            project_id,
            include_global: !args.project_only,
            project_only: args.project_only,
            global_only: args.global_only,
            memory_type,
            top_k: args.top_k,
            min_score: args.min_score,
        })),
        1,
    );
    
    let response = client.send(&request).await?;
    
    if args.human {
        print_human_output(&response);
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }
    
    Ok(())
}

fn print_human_output(response: &memrec_common::JsonRpcResponse) {
    if let Some(result) = &response.result {
        match result {
            memrec_common::ResponseResult::SemanticSearchResult(result) => {
                println!("Found {} memories:\n", result.total);
                
                for hit in &result.results {
                    println!("[{}] {} (score: {:.2})",
                        hit.memory_type.to_string().to_uppercase(),
                        truncate(&hit.content_preview, 50),
                        hit.score
                    );
                    println!("  ID: {}", hit.memory_id);
                    if let Some(pid) = hit.project_id {
                        if pid.is_nil() {
                            println!("  Project: (global)");
                        } else {
                            println!("  Project: {}", pid);
                        }
                    }
                    println!("  Tags: {:?}", hit.tags);
                    println!("  Created: {}", hit.created_at.format("%Y-%m-%d"));
                    
                    if hit.is_chunked {
                        println!("  Chunked memory ({}/{}). Use --merge.",
                            hit.chunk_index.unwrap_or(0) + 1,
                            hit.chunk_total.unwrap_or(0)
                        );
                    }
                    println!();
                }
            }
            _ => println!("Unexpected response type")
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        s.chars().take(max_len).collect::<String>() + "..."
    } else {
        s.to_string()
    }
}