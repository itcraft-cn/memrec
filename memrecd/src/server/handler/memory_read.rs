//! # 记忆读取处理器（Get/List/Delete/Restore/Stats）
//!
//! [`Router`] 的读取、列表、删除、恢复与统计方法。

use super::*;

impl Router {
    /// 处理 Get 请求：获取单条记忆。
    ///
    /// 若 `merge=true` 且记忆为分块记忆，合并所有分块返回完整内容。
    pub(super) async fn handle_get(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Get(p)) => {
                if p.merge {
                    self.handle_get_with_merge(&p.id, id).await
                } else {
                    match self.storage.get(&p.id).await {
                        Ok(Some(memory)) => {
                            self.storage.update(&memory).await.ok();
                            JsonRpcResponse::success(
                                ResponseResult::Memory(MemoryResult::new(memory)),
                                id,
                            )
                        }
                        Ok(None) => JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32001,
                                message: "Memory not found".to_string(),
                                data: None,
                            },
                            id,
                        ),
                        Err(e) => JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32000,
                                message: e.to_string(),
                                data: None,
                            },
                            id,
                        ),
                    }
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for Get".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 Get 请求（合并模式）：将分块记忆按 chunk_index 排序后拼接。
    pub(super) async fn handle_get_with_merge(&self, id: &Uuid, rpc_id: u64) -> JsonRpcResponse {
        match self.storage.get(id).await {
            Ok(Some(memory)) => {
                if !memory.is_chunked() {
                    self.storage.update(&memory).await.ok();
                    return JsonRpcResponse::success(
                        ResponseResult::Memory(MemoryResult::new(memory)),
                        rpc_id,
                    );
                }
                let group_id = memory.chunk_group_id.unwrap();
                match self.storage.get_chunks_by_group(&group_id).await {
                    Ok(chunks) => {
                        if chunks.is_empty() {
                            return JsonRpcResponse::error(
                                JsonRpcError {
                                    code: -32005,
                                    message: "No chunks found".to_string(),
                                    data: None,
                                },
                                rpc_id,
                            );
                        }
                        let mut sorted_chunks = chunks;
                        sorted_chunks.sort_by_key(|c| c.chunk_index.unwrap_or(0));
                        let merged_content = sorted_chunks
                            .iter()
                            .map(|c| c.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let first_chunk = sorted_chunks.first().unwrap();
                        let mut merged_memory =
                            Memory::new(merged_content, first_chunk.memory_type)
                                .with_tags(first_chunk.tags.clone());
                        merged_memory.id = group_id;
                        merged_memory.project_id = first_chunk.project_id;
                        JsonRpcResponse::success(
                            ResponseResult::Memory(MemoryResult::new(merged_memory)),
                            rpc_id,
                        )
                    }
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: e.to_string(),
                            data: None,
                        },
                        rpc_id,
                    ),
                }
            }
            Ok(None) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32001,
                    message: "Memory not found".to_string(),
                    data: None,
                },
                rpc_id,
            ),
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32000,
                    message: e.to_string(),
                    data: None,
                },
                rpc_id,
            ),
        }
    }

    /// 处理 List 请求：列出记忆，支持项目/全局过滤。
    pub(super) async fn handle_list(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let (
            limit,
            project_only,
            global_only,
            project_id,
            tags,
            memory_type,
            include_deleted,
            skip,
        ) = match params {
            Some(RequestParams::List(p)) => (
                p.limit,
                p.project_only,
                p.global_only,
                p.project_id,
                p.tags,
                p.memory_type,
                p.include_deleted,
                p.skip,
            ),
            _ => (20, false, false, None, Vec::new(), None, false, 0),
        };

        // 差分取数（P0/T1）：project/global 范围走 by_project 索引下推，
        // 避免全量加载后过滤；project_only 无具体项目、include_deleted 时
        // 无法用单一索引表达，回退全量（需要精确 total 以支撑分页）。
        let memories = if include_deleted {
            // 从 RocksDB cf_deleted 列族检索原始记录（含软删除）
            self.storage.list_deleted().await.unwrap_or_default()
        } else if project_only {
            match project_id {
                Some(pid) if !pid.is_nil() => {
                    self.storage.list_by_project(&pid).await.unwrap_or_default()
                }
                // project_only 不带 pid：语义为"全部项目内记忆"，无对应索引桶，回退全量
                _ => self.storage.list(usize::MAX).await.unwrap_or_default(),
            }
        } else if global_only {
            self.storage
                .list_global(usize::MAX)
                .await
                .unwrap_or_default()
        } else {
            self.storage.list(usize::MAX).await.unwrap_or_default()
        };

        let filtered: Vec<Memory> = memories
            .into_iter()
            .filter(|m| {
                if !include_deleted && m.is_deleted {
                    return false;
                }

                // 标签过滤（需全部命中）
                if !tags.is_empty() && !tags.iter().all(|t| m.tags.contains(t)) {
                    return false;
                }

                // 类型过滤
                if let Some(mt) = memory_type {
                    if m.memory_type != mt {
                        return false;
                    }
                }

                if project_only {
                    if let Some(pid) = project_id {
                        m.project_id == Some(pid)
                    } else {
                        m.project_id.is_some() && !m.project_id.unwrap().is_nil()
                    }
                } else if global_only {
                    m.project_id.is_none() || m.project_id.unwrap().is_nil()
                } else {
                    true
                }
            })
            .collect();

        // 分页：skip 跳过前 N 条，take 取 limit 条
        let total = filtered.len();
        let page: Vec<Memory> = filtered.into_iter().skip(skip).take(limit).collect();

        JsonRpcResponse::success(
            ResponseResult::MemoryList(MemoryListResult {
                memories: page,
                total,
            }),
            id,
        )
    }

    /// 处理 Delete 请求：删除记忆。
    ///
    /// 返回消息区分硬删除（已过恢复期）和软删除。
    pub(super) async fn handle_delete(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Delete(p)) => match self.storage.delete(&p.id).await {
                Ok(deleted) => {
                    // 同步移除搜索索引（向量 + 全文），避免已删记忆仍被检索命中
                    if let Err(e) = self.hybrid_store.remove(&p.id).await {
                        tracing::warn!(
                            "Delete: failed to remove search index for {:?}: {}",
                            p.id,
                            e
                        );
                    }
                    let message = if deleted {
                        "Memory hard deleted"
                    } else {
                        "Memory soft deleted"
                    };
                    JsonRpcResponse::success(
                        ResponseResult::Success(SuccessResult {
                            message: message.to_string(),
                        }),
                        id,
                    )
                }
                Err(e) => JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32000,
                        message: e.to_string(),
                        data: None,
                    },
                    id,
                ),
            },
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for Delete".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 恢复软删除记忆：置为活跃并重新加入混合检索索引。
    ///
    /// 仅能恢复软删除记录（`is_deleted=true`）；已硬删除的记录无法恢复。
    pub(super) async fn handle_restore(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        let target_id = match params {
            Some(RequestParams::Restore(p)) => p.id,
            _ => {
                return JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32602,
                        message: "Invalid params for Restore".to_string(),
                        data: None,
                    },
                    id,
                )
            }
        };

        match self.storage.restore(&target_id).await {
            Ok(Some(memory)) => {
                // 重新加入混合检索索引（向量 + 全文）
                if let Ok(embed) = self.embedder.embed(&memory.content) {
                    let payload = self.build_payload(&memory);
                    if let Err(e) = self
                        .hybrid_store
                        .add(&target_id, &embed, &memory.content, payload)
                        .await
                    {
                        tracing::warn!("Restore: failed to re-index {:?}: {}", target_id, e);
                    }
                }

                tracing::info!("Restored memory {:?}", target_id);
                JsonRpcResponse::success(ResponseResult::Memory(MemoryResult::new(memory)), id)
            }
            Ok(None) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32001,
                    message: "Memory is not deleted, nothing to restore".to_string(),
                    data: None,
                },
                id,
            ),
            Err(e) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32000,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 处理 Stats 请求：返回记忆统计信息。
    pub(super) async fn handle_stats(&self, id: u64) -> JsonRpcResponse {
        match (
            self.storage.count().await,
            self.storage.count_deleted().await,
        ) {
            (Ok(total), Ok(deleted)) => JsonRpcResponse::success(
                ResponseResult::Stats(StatsResult {
                    total_memories: total + deleted,
                    active_memories: total,
                    deleted_memories: deleted,
                }),
                id,
            ),
            (Err(e), _) | (_, Err(e)) => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32000,
                    message: e.to_string(),
                    data: None,
                },
                id,
            ),
        }
    }
}
