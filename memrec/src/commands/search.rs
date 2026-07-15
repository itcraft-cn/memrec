//! # 语义搜索命令
//!
//! 通过向量相似度搜索记忆，支持项目/全局/跨项目过滤。

use crate::client::Client;
use clap::Args;
use memrec_common::{JsonRpcRequest, MemoryType, RequestAction, RequestParams, SearchMemoryParams};
use uuid::Uuid;

/// 搜索命令参数
///
/// | 参数 | 说明 | 默认值 |
/// |------|------|--------|
/// | `query` | 搜索查询（必填） | — |
/// | `top_k` | 返回数量 | 10 |
/// | `min_score` | 最低相似度 | 0.5 |
/// | `project_only` | 仅当前项目 | false |
/// | `global_only` | 仅全局记忆 | false |
/// | `all` | 跨项目搜索 | false |
/// | `mtype` | 记忆类型过滤 | None |
#[derive(Args, Debug)]
pub struct SearchArgs {
    #[arg(required = true)]
    pub query: String,

    #[arg(short = 'k', long, default_value = "10")]
    pub top_k: usize,

    #[arg(long, default_value = "0.5")]
    pub min_score: f32,

    #[arg(long)]
    pub project_only: bool,

    #[arg(long)]
    pub global_only: bool,

    #[arg(long)]
    pub all: bool,

    #[arg(long)]
    pub mtype: Option<String>,
}

/// 执行语义搜索，发送请求到守护进程并输出结果。
pub async fn search_execute(
    client: &Client,
    args: SearchArgs,
    working_dir: Option<String>,
    human: bool,
) -> anyhow::Result<()> {
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
            cross_project: args.all,
            memory_type,
            top_k: args.top_k,
            min_score: args.min_score,
            working_dir,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        print_human_output(&response);
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// 人类可读格式输出搜索结果。
fn print_human_output(response: &memrec_common::JsonRpcResponse) {
    if let Some(result) = &response.result {
        match result {
            memrec_common::ResponseResult::SemanticSearchResult(result) => {
                println!("Found {} memories:\n", result.total);

                for hit in &result.results {
                    println!(
                        "[{}] {} (score: {:.2})",
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
                        println!(
                            "  Chunked memory ({}/{}). Use --merge.",
                            hit.chunk_index.unwrap_or(0) + 1,
                            hit.chunk_total.unwrap_or(0)
                        );
                    }
                    println!();
                }
            }
            _ => println!("Unexpected response type"),
        }
    }
}

/// 截断字符串，超出部分用 `...` 替代。
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        s.chars().take(max_len).collect::<String>() + "..."
    } else {
        s.to_string()
    }
}
