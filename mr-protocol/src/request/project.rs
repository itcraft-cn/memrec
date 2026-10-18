//! 项目与配置相关请求参数。

use serde::{Deserialize, Serialize};

/// 获取当前项目信息的参数。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetProjectInfoParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

/// 获取服务版本的参数（当前无字段，保留扩展性）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetVersionParams;

/// 创建项目的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCreateParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 切换当前活跃项目的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSwitchParams {
    pub name: String,
}

/// 删除项目的参数。
///
/// `force` 为 true 时跳过确认直接删除。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDeleteParams {
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

/// 列出项目参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectListParams {}

/// 获取配置项参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigGetParams {
    pub key: String,
}

/// 设置配置项的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSetParams {
    pub key: String,
    pub value: String,
}

