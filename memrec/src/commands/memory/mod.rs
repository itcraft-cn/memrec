//! # CLI 记忆命令（分组子模块）
//!
//! 原单文件约 620 行，现拆为 crud（增删改查/统计/版本）、
//! chunk（长内容分块）、dream（整合）与单元测试。

use crate::client::Client;
use anyhow::Result;
use mr_common::MemoryType;
use mr_protocol::{
    AddParams, DeleteParams, DreamParams, DreamReportParams, GetParams, JsonRpcRequest, ListParams,
    RequestAction, RequestParams, ResponseResult, RestoreParams,
};

/// 单块最大字节数，留余量给 JSON 序列化开销
const MAX_CHUNK_SIZE: usize = 7500;

mod chunk;
mod crud;
mod dream;

pub use crud::{add, delete, get, list, restore, stats, version, AddOptions, ListOptions};
pub use dream::{dream, dream_report};

#[cfg(test)]
mod tests;
