//! 场景相关请求参数。

use serde::{Deserialize, Serialize};
use super::memory::default_limit;
use super::*;

/// 创建场景参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneCreateParams {
    pub theme: String,
    pub project_id: Option<Uuid>,
    pub tags: Vec<String>,
}

/// 获取场景参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneGetParams {
    pub id: Uuid,
}

/// 列出场景参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneListParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub sort_by_heat: bool,
    pub project_id: Option<Uuid>,
}

/// 删除场景参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneDeleteParams {
    pub id: Uuid,
}

/// 添加记忆到场景参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAddMemoryParams {
    pub scene_id: Uuid,
    pub memory_id: Uuid,
}

/// 从场景移除记忆参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneRemoveMemoryParams {
    pub scene_id: Uuid,
    pub memory_id: Uuid,
}

/// 更新场景热度参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneUpdateHeatParams {
    pub scene_id: Uuid,
    pub heat: f32,
}

