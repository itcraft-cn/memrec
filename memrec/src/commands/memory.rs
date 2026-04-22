use anyhow::Result;
use memrec_common::{
    JsonRpcRequest, protocol::{RequestAction, RequestParams, ResponseResult,
    AddParams, GetParams, DeleteParams, ListParams},
    MemoryType,
};
use crate::client::Client;

pub async fn add(client: &Client, content: String, mtype: String, tags: Vec<String>) -> Result<()> {
    let memory_type = parse_memory_type(&mtype)?;
    
    let request = JsonRpcRequest::new(
        RequestAction::Add,
        Some(RequestParams::Add(AddParams {
            content,
            memory_type,
            tags,
            project_id: None,
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
    
    Ok(())
}

pub async fn get(client: &Client, id: String) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| anyhow::anyhow!("Invalid UUID: {}", e))?;
    
    let request = JsonRpcRequest::new(
        RequestAction::Get,
        Some(RequestParams::Get(GetParams { id: uuid })),
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

pub async fn list(client: &Client, limit: usize) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::List,
        Some(RequestParams::List(ListParams {
            tags: Vec::new(),
            memory_type: None,
            limit,
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