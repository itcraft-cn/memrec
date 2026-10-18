//! 记忆增删改查、列表、统计与版本命令。

use super::chunk::*;
use super::*;

/// `add` 命令参数集合
pub struct AddOptions {
    pub content: String,
    pub mtype: String,
    pub tags: Vec<String>,
    pub is_global: bool,
    pub working_dir: Option<String>,
    pub source: Option<String>,
    pub scope: Option<String>,
    /// 去重检测开关：`None` 用服务端配置，`Some(false)` 显式关闭
    pub dedup: Option<bool>,
}

/// 添加记忆命令。
///
/// 内容超长时自动分块，每块独立发送 Add 请求。
/// 默认输出 JSON-RPC 响应，`human=true` 时输出人类可读格式。
pub async fn add(client: &Client, opts: AddOptions, human: bool) -> Result<()> {
    let AddOptions {
        content,
        mtype,
        tags,
        is_global,
        working_dir,
        source,
        scope,
        dedup,
    } = opts;
    let memory_type = parse_memory_type(&mtype)?;

    if content.len() > MAX_CHUNK_SIZE {
        if human {
            eprintln!(
                "WARN: Content too long ({:.1}KB > {:.1}KB), auto-splitting into chunks...",
                content.len() as f64 / 1024.0,
                MAX_CHUNK_SIZE as f64 / 1024.0
            );
        }

        let chunks = split_content(&content, MAX_CHUNK_SIZE);
        if human {
            eprintln!("WARN: Split into {} parts", chunks.len());
        }

        let mut ids = Vec::new();
        let mut last_response = None;
        for (i, chunk) in chunks.iter().enumerate() {
            let part_tags = format_part_tags(&tags, i + 1, chunks.len());

            let request = JsonRpcRequest::new(
                RequestAction::Add,
                Some(RequestParams::Add(AddParams {
                    content: chunk.clone(),
                    memory_type,
                    tags: part_tags,
                    project_id: None,
                    is_global,
                    working_dir: working_dir.clone(),
                    source: source.clone(),
                    scope: scope.clone(),
                    dedup,
                })),
                i as u64 + 1,
            );

            let response = client.send(&request).await?;
            last_response = Some(response.clone());

            if let Some(ResponseResult::Memory(m)) = response.result {
                ids.push(m.memory.id);
                if human {
                    println!("Part {}: Added {}", i + 1, m.memory.id);
                }
            }
        }

        if human {
            println!("All {} parts added: {:?}", ids.len(), ids);
        } else if let Some(resp) = last_response {
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
    } else {
        let request = JsonRpcRequest::new(
            RequestAction::Add,
            Some(RequestParams::Add(AddParams {
                content,
                memory_type,
                tags,
                project_id: None,
                is_global,
                working_dir,
                source,
                scope,
                dedup,
            })),
            1,
        );

        let response = client.send(&request).await?;

        if human {
            if let Some(result) = response.result {
                match result {
                    ResponseResult::Memory(m) => match m.action.as_str() {
                        "skipped" => {
                            println!("Skipped (duplicate): {}", m.memory.id);
                            for d in &m.duplicates {
                                println!(
                                    "  similar to {:?} (score={:.3}): {}",
                                    d.memory_id, d.score, d.content
                                );
                            }
                        }
                        "updated" => {
                            println!("Updated existing memory: {}", m.memory.id);
                            println!("Content: {}", m.memory.content);
                            println!("Tags: {:?}", m.memory.tags);
                        }
                        _ => {
                            println!("Added memory: {}", m.memory.id);
                            println!("Content: {}", m.memory.content);
                            println!("Type: {:?}", m.memory.memory_type);
                            println!("Tags: {:?}", m.memory.tags);
                            for d in &m.duplicates {
                                println!(
                                    "  WARN: similar memory {:?} (score={:.3})",
                                    d.memory_id, d.score
                                );
                            }
                        }
                    },
                    _ => println!("Unexpected response type"),
                }
            } else if let Some(err) = response.error {
                println!("Error: {}", err.message);
            }
        } else {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
    }

    Ok(())
}

/// 获取记忆命令，支持分块合并（`--merge`）。
pub async fn get(client: &Client, id: String, merge: bool, human: bool) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?;

    let request = JsonRpcRequest::new(
        RequestAction::Get,
        Some(RequestParams::Get(GetParams { id: uuid, merge })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        if let Some(result) = response.result {
            match result {
                ResponseResult::Memory(m) => {
                    println!("Memory ID: {}", m.memory.id);
                    println!("Content: {}", m.memory.content);
                    println!("Type: {:?}", m.memory.memory_type);
                    println!("Importance: {:.2}", m.memory.importance);
                    println!("Tags: {:?}", m.memory.tags);
                    println!("Created: {}", m.memory.created_at);
                    println!("Access count: {}", m.memory.access_count);
                    if m.memory.is_deleted {
                        println!("Status: DELETED");
                    }
                }
                _ => println!("Unexpected response type"),
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// 列出记忆命令，支持项目/全局过滤。
/// `list` 命令参数集合
pub struct ListOptions {
    pub limit: usize,
    /// 跳过前 N 条（与 limit 配合实现分页）
    pub skip: usize,
    /// 仅列出指定项目（由 `-p` 解析 `.mr_pid` 得到）
    pub project_only: bool,
    pub global_only: bool,
    /// 项目 UUID：`project_only` 时限定到具体项目，`None` 则回退到"全部非全局"
    pub project_id: Option<uuid::Uuid>,
    /// 按标签过滤（可多个，全部命中）
    pub tags: Vec<String>,
    /// 按记忆类型过滤
    pub memory_type: Option<String>,
    /// 包含已删除（软删除）的原始记录
    pub include_deleted: bool,
}

/// 列出记忆命令。
///
/// 支持标签/类型过滤与 `skip` 分页，`include_deleted` 时从 RocksDB 检索软删除的原始记录。
pub async fn list(client: &Client, opts: ListOptions, human: bool) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::List,
        Some(RequestParams::List(ListParams {
            tags: opts.tags,
            memory_type: opts
                .memory_type
                .map(|t| parse_memory_type(&t))
                .transpose()?,
            limit: opts.limit,
            project_only: opts.project_only,
            global_only: opts.global_only,
            project_id: opts.project_id,
            include_deleted: opts.include_deleted,
            skip: opts.skip,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        if let Some(result) = response.result {
            match result {
                ResponseResult::MemoryList(m) => {
                    println!("Found {} memories (total: {})", m.memories.len(), m.total);
                    for memory in m.memories {
                        println!(
                            "\n[{:?}] {}...",
                            memory.memory_type,
                            memory.content.chars().take(50).collect::<String>()
                        );
                        println!("  ID: {}", memory.id);
                        println!("  Tags: {:?}", memory.tags);
                        println!("  Importance: {:.2}", memory.importance);
                    }
                }
                _ => println!("Unexpected response type"),
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// 删除记忆命令（软删除）。
pub async fn delete(client: &Client, id: String, human: bool) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?;

    let request = JsonRpcRequest::new(
        RequestAction::Delete,
        Some(RequestParams::Delete(DeleteParams {
            id: uuid,
            force: false,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        if let Some(result) = response.result {
            match result {
                ResponseResult::Success(s) => {
                    println!("{}", s.message);
                }
                _ => println!("Unexpected response type"),
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// 恢复软删除记忆命令。
///
/// 将 `is_deleted=true` 的原始记录恢复为活跃状态，并重新加入混合检索索引。
pub async fn restore(client: &Client, id: String, human: bool) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?;

    let request = JsonRpcRequest::new(
        RequestAction::Restore,
        Some(RequestParams::Restore(RestoreParams { id: uuid })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        if let Some(result) = response.result {
            match result {
                ResponseResult::Memory(m) => {
                    println!("Restored memory {}:", m.memory.id);
                    println!(
                        "  {}",
                        m.memory.content.chars().take(100).collect::<String>()
                    );
                    println!("  Tags: {:?}", m.memory.tags);
                }
                _ => println!("Unexpected response type"),
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// 统计信息命令。
pub async fn stats(client: &Client, human: bool) -> Result<()> {
    let request = JsonRpcRequest::new(RequestAction::Stats, None, 1);

    let response = client.send(&request).await?;

    if human {
        if let Some(result) = response.result {
            match result {
                ResponseResult::Stats(s) => {
                    println!("Memory Statistics:");
                    println!("  Total memories: {}", s.total_memories);
                    println!("  Active memories: {}", s.active_memories);
                    println!("  Deleted memories: {}", s.deleted_memories);
                }
                _ => println!("Unexpected response type"),
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// 版本号命令，对比 CLI 和守护进程版本。
pub async fn version(client: &Client, human: bool) -> Result<()> {
    let cli_version = env!("CARGO_PKG_VERSION");

    let request = JsonRpcRequest::new(RequestAction::GetVersion, None, 1);

    let response = client.send(&request).await?;

    if human {
        let server_version = if let Some(result) = response.result {
            match result {
                ResponseResult::Version(v) => v.version,
                _ => {
                    println!("Error: Unexpected response type");
                    return Ok(());
                }
            }
        } else if let Some(err) = response.error {
            println!("Error: Failed to get server version: {}", err.message);
            return Ok(());
        } else {
            println!("Error: No response from server");
            return Ok(());
        };

        println!("CLI version:    {}", cli_version);
        println!("Server version: {}", server_version);

        if cli_version != server_version {
            println!("\n⚠️  WARNING: Version mismatch!");
            println!("Please update memrecd to match CLI version.");
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// 解析记忆类型字符串。
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

