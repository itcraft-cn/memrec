//! # 场景管理命令
//!
//! 实现 `scene create`、`scene get`、`scene list`、`scene delete`、`scene add-memory`、`scene remove-memory` 子命令。

use crate::client::Client;
use anyhow::Result;
use mr_protocol::{
    JsonRpcRequest, RequestAction, RequestParams, ResponseResult, SceneAddMemoryParams,
    SceneCreateParams, SceneDeleteParams, SceneGetParams, SceneListParams, SceneRemoveMemoryParams,
    SceneUpdateHeatParams,
};
use uuid::Uuid;

/// 创建场景命令。
pub async fn create(
    client: &Client,
    theme: String,
    project_id: Option<Uuid>,
    tags: Vec<String>,
    human: bool,
) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::SceneCreate,
        Some(RequestParams::SceneCreate(SceneCreateParams {
            theme,
            project_id,
            tags,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        match response.result {
            Some(ResponseResult::Scene(result)) => {
                println!("Scene created: {}", result.scene.id);
                println!("  Theme: {}", result.scene.theme);
                println!("  Heat: {:.4}", result.scene.heat);
                println!("  Memories: {}", result.scene.memory_ids.len());
            }
            _ => eprintln!("Unexpected response type"),
        }
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

/// 获取场景命令。
pub async fn get(client: &Client, id: Uuid, human: bool) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::SceneGet,
        Some(RequestParams::SceneGet(SceneGetParams { id })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        match response.result {
            Some(ResponseResult::Scene(result)) => {
                println!("Scene: {}", result.scene.id);
                println!("  Theme: {}", result.scene.theme);
                println!("  Heat: {:.4}", result.scene.heat);
                println!("  Tags: {:?}", result.scene.tags);
                println!("  Memories: {} items", result.scene.memory_ids.len());
                for (i, memory_id) in result.scene.memory_ids.iter().enumerate() {
                    println!("    [{}] {}", i + 1, memory_id);
                }
                println!("  Created: {}", result.scene.created_at);
                println!("  Updated: {}", result.scene.updated_at);
                if result.scene.is_deleted {
                    println!("  [DELETED] at {:?}", result.scene.deleted_at);
                }
            }
            Some(ResponseResult::Success(success)) => {
                println!("{}", success.message);
            }
            _ => eprintln!("Unexpected response type"),
        }
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

/// 列出场景命令。
pub async fn list(
    client: &Client,
    limit: usize,
    sort_by_heat: bool,
    project_id: Option<Uuid>,
    human: bool,
) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::SceneList,
        Some(RequestParams::SceneList(SceneListParams {
            limit,
            sort_by_heat,
            project_id,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        match response.result {
            Some(ResponseResult::SceneList(result)) => {
                println!("Found {} scenes:", result.total);
                for (i, scene) in result.scenes.iter().enumerate() {
                    println!("\n[{}] {} - {}", i + 1, scene.id, scene.theme);
                    println!("  Heat: {:.4}", scene.heat);
                    println!("  Memories: {}", scene.memory_ids.len());
                    println!("  Tags: {:?}", scene.tags);
                }
            }
            _ => eprintln!("Unexpected response type"),
        }
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

/// 删除场景命令。
pub async fn delete(client: &Client, id: Uuid, human: bool) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::SceneDelete,
        Some(RequestParams::SceneDelete(SceneDeleteParams { id })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        match response.result {
            Some(ResponseResult::Success(success)) => {
                println!("{}", success.message);
            }
            _ => eprintln!("Unexpected response type"),
        }
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

/// 添加记忆到场景命令。
pub async fn add_memory(
    client: &Client,
    scene_id: Uuid,
    memory_id: Uuid,
    human: bool,
) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::SceneAddMemory,
        Some(RequestParams::SceneAddMemory(SceneAddMemoryParams {
            scene_id,
            memory_id,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        match response.result {
            Some(ResponseResult::Success(success)) => {
                println!("{}", success.message);
            }
            _ => eprintln!("Unexpected response type"),
        }
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

/// 从场景移除记忆命令。
pub async fn remove_memory(
    client: &Client,
    scene_id: Uuid,
    memory_id: Uuid,
    human: bool,
) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::SceneRemoveMemory,
        Some(RequestParams::SceneRemoveMemory(SceneRemoveMemoryParams {
            scene_id,
            memory_id,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        match response.result {
            Some(ResponseResult::Success(success)) => {
                println!("{}", success.message);
            }
            _ => eprintln!("Unexpected response type"),
        }
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

/// 更新场景热度命令。
pub async fn update_heat(client: &Client, scene_id: Uuid, heat: f32, human: bool) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::SceneUpdateHeat,
        Some(RequestParams::SceneUpdateHeat(SceneUpdateHeatParams {
            scene_id,
            heat,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        match response.result {
            Some(ResponseResult::Success(success)) => {
                println!("{}", success.message);
            }
            _ => eprintln!("Unexpected response type"),
        }
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}
