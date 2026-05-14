use anyhow::Result;
use memrec_common::{
    JsonRpcRequest, protocol::{RequestAction, RequestParams, ResponseResult,
    AddParams, GetParams, DeleteParams, ListParams},
    MemoryType,
};
use crate::client::Client;

const MAX_CHUNK_SIZE: usize = 7500;  // 留余量给JSON序列化

pub async fn add(client: &Client, content: String, mtype: String, tags: Vec<String>, is_global: bool, working_dir: Option<String>) -> Result<()> {
    let memory_type = parse_memory_type(&mtype)?;
    
    if content.len() > MAX_CHUNK_SIZE {
        eprintln!("WARN: Content too long ({:.1}KB > {:.1}KB), auto-splitting into chunks...",
            content.len() as f64 / 1024.0,
            MAX_CHUNK_SIZE as f64 / 1024.0
        );
        
        let chunks = split_content(&content, MAX_CHUNK_SIZE);
        eprintln!("WARN: Split into {} parts", chunks.len());
        
        let mut ids = Vec::new();
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
                })),
                i as u64 + 1,
            );
            
            let response = client.send(&request).await?;
            
            if let Some(ResponseResult::Memory(m)) = response.result {
                ids.push(m.memory.id);
                println!("Part {}: Added {}", i + 1, m.memory.id);
            }
        }
        
        println!("All {} parts added: {:?}", ids.len(), ids);
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
            })),
            1,
        );
        
        let response = client.send(&request).await?;
        
        if let Some(result) = response.result {
            match result {
                ResponseResult::Memory(m) => {
                    println!("Added memory: {}", m.memory.id);
                    println!("Content: {}", m.memory.content);
                    println!("Type: {:?}", m.memory.memory_type);
                    println!("Tags: {:?}", m.memory.tags);
                }
                _ => println!("Unexpected response type")
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    }
    
    Ok(())
}

fn split_content(content: &str, max_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    
    while start < content.len() {
        let end = find_chunk_boundary(content, start, max_size);
        chunks.push(content[start..end].to_string());
        start = end;
    }
    
    chunks
}

fn find_chunk_boundary(content: &str, start: usize, max_size: usize) -> usize {
    let max_end = std::cmp::min(start + max_size, content.len());
    
    if max_end == content.len() {
        return max_end;
    }
    
    // 尝试在句子边界拆分（优先）
    let chunk = &content[start..max_end];
    let sentence_ends = ['.', '!', '?', '\n', '。', '！', '？'];
    
    for end_char in sentence_ends.iter().rev() {
        if let Some(pos) = chunk.rfind(*end_char) {
            let boundary = start + pos + end_char.len_utf8();
            if boundary > start + 1000 {  // 至少1KB
                return boundary;
            }
        }
    }
    
    // 按UTF-8字符边界拆分
    let mut boundary = max_end;
    while boundary > start && !content.is_char_boundary(boundary) {
        boundary -= 1;
    }
    
    boundary
}

fn format_part_tags(original_tags: &[String], part_num: usize, total_parts: usize) -> Vec<String> {
    let mut tags = original_tags.to_vec();
    tags.push(format!("part:{}-{}", part_num, total_parts));
    if part_num == 1 {
        tags.push("part:first".to_string());
    }
    if part_num == total_parts {
        tags.push("part:last".to_string());
    }
    tags
}

pub async fn get(client: &Client, id: String, merge: bool) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?;
    
    let request = JsonRpcRequest::new(
        RequestAction::Get,
        Some(RequestParams::Get(GetParams { id: uuid, merge })),
        1,
    );
    
    let response = client.send(&request).await?;
    
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
            _ => println!("Unexpected response type")
        }
    } else if let Some(err) = response.error {
        println!("Error: {}", err.message);
    }
    
    Ok(())
}

pub async fn list(client: &Client, limit: usize, project_only: bool, global_only: bool) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::List,
        Some(RequestParams::List(ListParams {
            tags: Vec::new(),
            memory_type: None,
            limit,
            project_only,
            global_only,
            project_id: None,
        })),
        1,
    );
    
    let response = client.send(&request).await?;
    
    if let Some(result) = response.result {
        match result {
            ResponseResult::MemoryList(m) => {
                println!("Found {} memories (total: {})", m.memories.len(), m.total);
                for memory in m.memories {
                    println!("\n[{:?}] {}...", 
                        memory.memory_type, 
                        &memory.content.chars().take(50).collect::<String>()
                    );
                    println!("  ID: {}", memory.id);
                    println!("  Tags: {:?}", memory.tags);
                    println!("  Importance: {:.2}", memory.importance);
                }
            }
            _ => println!("Unexpected response type")
        }
    } else if let Some(err) = response.error {
        println!("Error: {}", err.message);
    }
    
    Ok(())
}

pub async fn delete(client: &Client, id: String) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?;
    
    let request = JsonRpcRequest::new(
        RequestAction::Delete,
        Some(RequestParams::Delete(DeleteParams { id: uuid, force: false })),
        1,
    );
    
    let response = client.send(&request).await?;
    
    if let Some(result) = response.result {
        match result {
            ResponseResult::Success(s) => {
                println!("{}", s.message);
            }
            _ => println!("Unexpected response type")
        }
    } else if let Some(err) = response.error {
        println!("Error: {}", err.message);
    }
    
    Ok(())
}

pub async fn stats(client: &Client) -> Result<()> {
    let request = JsonRpcRequest::new(RequestAction::Stats, None, 1);
    
    let response = client.send(&request).await?;
    
    if let Some(result) = response.result {
        match result {
            ResponseResult::Stats(s) => {
                println!("Memory Statistics:");
                println!("  Total memories: {}", s.total_memories);
                println!("  Active memories: {}", s.active_memories);
                println!("  Deleted memories: {}", s.deleted_memories);
            }
            _ => println!("Unexpected response type")
        }
    } else if let Some(err) = response.error {
        println!("Error: {}", err.message);
    }
    
    Ok(())
}

pub async fn version(client: &Client) -> Result<()> {
    let cli_version = env!("CARGO_PKG_VERSION");
    
    let request = JsonRpcRequest::new(RequestAction::GetVersion, None, 1);
    
    let response = client.send(&request).await?;
    
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
    
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_split_short_content() {
        let content = "short content".to_string();
        let chunks = split_content(&content, 7500);
        assert_eq!(chunks.len(), 1);
    }
    
    #[test]
    fn test_split_long_content() {
        let content = "a".repeat(15000);
        let chunks = split_content(&content, 7500);
        assert_eq!(chunks.len(), 2);
    }
    
    #[test]
    fn test_split_preserves_utf8() {
        let content = "中文测试内容".repeat(2000);
        let chunks = split_content(&content, 7500);
        
        for chunk in &chunks {
            assert!(chunk.is_char_boundary(chunk.len()));
        }
    }
    
    #[test]
    fn test_format_part_tags() {
        let tags = vec!["rust".to_string()];
        let result = format_part_tags(&tags, 1, 3);
        
        assert!(result.contains(&"rust".to_string()));
        assert!(result.contains(&"part:1-3".to_string()));
        assert!(result.contains(&"part:first".to_string()));
    }
}