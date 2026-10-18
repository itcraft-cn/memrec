//! # 语义搜索命令
//!
//! 通过向量相似度搜索记忆，支持项目/全局/跨项目过滤。

use crate::client::Client;
use crate::commands::Scope;
use clap::Args;
use mr_common::MemoryType;
use mr_protocol::{
    default_hybrid_alpha, default_mmr_enabled, default_mmr_lambda, JsonRpcRequest, RequestAction,
    RequestParams, SearchMemoryParams,
};

/// 搜索命令参数
///
/// | 参数 | 说明 | 默认值 |
/// |------|------|--------|
/// | `query` | 搜索查询（必填） | — |
/// | `top_k` | 返回数量 | 10 |
/// | `min_score` | 最低相似度 | 0.5 |
/// | `project` | 仅当前项目（`-p`） | 按 `.mr_pid` 判断 |
/// | `global` | 仅全局记忆（`-g`） | 按 `.mr_pid` 判断 |
/// | `all` | 跨项目搜索（`-a`） | false |
/// | `mtype` | 记忆类型过滤 | None |
#[derive(Args, Debug)]
pub struct SearchArgs {
    #[arg(required = true)]
    pub query: String,

    #[arg(short = 'k', long, default_value = "10")]
    pub top_k: usize,

    #[arg(long, default_value = "0.5")]
    pub min_score: f32,

    /// 仅当前项目
    #[arg(short = 'p', long, group = "scope")]
    pub project: bool,

    /// 仅公共记忆
    #[arg(short = 'g', long, group = "scope")]
    pub global: bool,

    /// 跨所有项目
    #[arg(short = 'a', long, group = "scope")]
    pub all: bool,

    #[arg(long)]
    pub mtype: Option<String>,
}

/// 执行语义搜索，发送请求到守护进程并输出结果。
///
/// `scope` 为解析后的检索范围（`-p/-g/-a` 或按 `.mr_pid` 推断的默认范围），
/// `working_dir` 仅在项目范围时传入（供服务端定位项目 UUID）。
pub async fn search_execute(
    client: &Client,
    args: SearchArgs,
    scope: Scope,
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

    let (project_only, global_only, cross_project, project_id) = match scope {
        Scope::Project => (true, false, false, None),
        Scope::Global => (false, true, false, None),
        Scope::All => (false, false, true, None),
    };

    let request = JsonRpcRequest::new(
        RequestAction::SearchMemory,
        Some(RequestParams::SearchMemory(SearchMemoryParams {
            query: args.query,
            project_id,
            include_global: false,
            project_only,
            global_only,
            cross_project,
            memory_type,
            top_k: args.top_k,
            min_score: args.min_score,
            working_dir,
            hybrid_alpha: default_hybrid_alpha(),
            mmr_enabled: default_mmr_enabled(),
            mmr_lambda: default_mmr_lambda(),
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
fn print_human_output(response: &mr_protocol::JsonRpcResponse) {
    if let Some(result) = &response.result {
        match result {
            mr_protocol::ResponseResult::SemanticSearchResult(result) => {
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
