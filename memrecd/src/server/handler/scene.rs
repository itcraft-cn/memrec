//! # 场景处理器
//!
//! [`Router`] 的 Scene* 方法（创建/获取/列表/删除/增删记忆/热度）。

use super::*;

impl Router {
    /// 处理 SceneCreate 请求：创建场景。
    pub(super) async fn handle_scene_create(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneCreate(p)) => {
                let mut scene = mr_common::Scene::new(p.theme);
                if let Some(project_id) = p.project_id {
                    scene.project_id = Some(project_id);
                }
                scene.tags = p.tags;

                match self.scene_store.save_scene(&scene).await {
                    Ok(_) => {
                        JsonRpcResponse::success(ResponseResult::Scene(SceneResult { scene }), id)
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to create scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneCreate".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneGet 请求：获取场景。
    pub(super) async fn handle_scene_get(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneGet(p)) => match self.scene_store.get_scene(&p.id).await {
                Ok(Some(scene)) => {
                    JsonRpcResponse::success(ResponseResult::Scene(SceneResult { scene }), id)
                }
                Ok(None) => JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32001,
                        message: "Scene not found".to_string(),
                        data: None,
                    },
                    id,
                ),
                Err(e) => JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32000,
                        message: format!("Failed to get scene: {}", e),
                        data: None,
                    },
                    id,
                ),
            },
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneGet".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneList 请求：列出场景。
    pub(super) async fn handle_scene_list(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneList(p)) => {
                match self
                    .scene_store
                    .list_scenes(p.limit, p.sort_by_heat, p.project_id)
                    .await
                {
                    Ok(scenes) => {
                        let total = scenes.len();
                        JsonRpcResponse::success(
                            ResponseResult::SceneList(SceneListResult { scenes, total }),
                            id,
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to list scenes: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneList".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneDelete 请求：删除场景。
    pub(super) async fn handle_scene_delete(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneDelete(p)) => {
                match self.scene_store.delete_scene(&p.id).await {
                    Ok(true) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Scene deleted".to_string(),
                        }),
                        id,
                    ),
                    Ok(false) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Scene marked as deleted".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to delete scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneDelete".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneAddMemory 请求：添加记忆到场景。
    pub(super) async fn handle_scene_add_memory(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneAddMemory(p)) => {
                match self
                    .scene_store
                    .add_memory_to_scene(&p.scene_id, &p.memory_id)
                    .await
                {
                    Ok(_) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Memory added to scene".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to add memory to scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneAddMemory".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneRemoveMemory 请求：从场景移除记忆。
    pub(super) async fn handle_scene_remove_memory(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneRemoveMemory(p)) => {
                match self
                    .scene_store
                    .remove_memory_from_scene(&p.scene_id, &p.memory_id)
                    .await
                {
                    Ok(_) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Memory removed from scene".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to remove memory from scene: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneRemoveMemory".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 SceneUpdateHeat 请求：更新场景热度。
    pub(super) async fn handle_scene_update_heat(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SceneUpdateHeat(p)) => {
                match self
                    .scene_store
                    .update_scene_heat(&p.scene_id, p.heat)
                    .await
                {
                    Ok(_) => JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: "Scene heat updated".to_string(),
                        }),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("Failed to update scene heat: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SceneUpdateHeat".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }
}
