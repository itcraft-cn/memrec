//! Dream 相关请求参数。

use serde::{Deserialize, Serialize};

/// Dream 记忆整合的参数。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DreamParams {
    /// 强制触发（跳过年/时间/新记忆门控）
    #[serde(default)]
    pub force: bool,
    /// 预演：只计算不写库、不删数据、不建边（便于安全查看 Dream 将做什么）
    #[serde(default)]
    pub dry_run: bool,
}

/// 查询 Dream 运行审计报告（DreamRun）的参数。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DreamReportParams {
    /// 返回最近若干条（默认 10，最大 100）
    #[serde(default)]
    pub last: Option<usize>,
    /// 起始偏移（分页）
    #[serde(default)]
    pub offset: Option<usize>,
}

