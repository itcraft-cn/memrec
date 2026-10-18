//! # 记忆写入处理器（Add 与去重）
//!
//! [`Router`] 的 Add 与写入去重方法（T6 拆分后的写入路径）。

use super::*;

impl Router {
    /// 处理 Add 请求：保存记忆并生成嵌入向量。
    ///
    /// 全局记忆使用 `project_id = None`（T4 语义）；项目记忆自动检测项目 ID。
    /// 开启去重检测时，先检索相似记忆并按规则决策：
    /// - 重复（相似度 >= duplicate_threshold）→ `skipped`，不写入
    /// - 可更新（>= update_threshold）→ `updated`，合并更新已有记忆
    /// - 否则正常写入 → `added`
    ///
    /// T6: 原单方法约 200 行，拆为 记忆构建 / 去重决策 / 索引重建 / 持久化
    /// 四个独立环节，`handle_add` 仅做分发与响应包装。
    pub(super) async fn handle_add(&self, params: Option<RequestParams>, id: u64) -> JsonRpcResponse {
        match params {
            Some(RequestParams::Add(p)) => {
                let memory = self.build_add_memory(&p);

                // 去重检测：调用方可通过 dedup=false 显式关闭，默认遵循配置。
                // 返回 Some(response) 表示去重决策已终结本次请求；
                // 窗口外修正路径落在 decision 内部标注 superseded 后返回 None，继续正常新增。
                let dedup_enabled = p.dedup.unwrap_or(self.dedup_config.enabled);
                if dedup_enabled {
                    if let Some(decision) = self.check_dedup(&p.content).await {
                        if let Some(resp) =
                            self.apply_dedup_decision(&p, &memory, decision, id).await
                        {
                            return resp;
                        }
                    }
                }

                // 正常新增：save + 归档 + 嵌入索引；失败按 -32000 返回
                if let Err(e) = self.save_and_index(&memory, &p.content).await {
                    return JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32000,
                            message: format!("{:#}", e),
                            data: None,
                        },
                        id,
                    );
                }

                JsonRpcResponse::success(
                    ResponseResult::Memory(MemoryResult {
                        memory,
                        action: "added".to_string(),
                        duplicates: Vec::new(),
                    }),
                    id,
                )
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for Add".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    /// 由 Add 请求参数构建记忆实体：归属/来源/范围按 T4 语义落定。
    pub(super) fn build_add_memory(&self, p: &mr_protocol::AddParams) -> Memory {
        // T4: 全局记忆 = project_id None（历史 Some(nil) 语义在启动迁移中归一）
        let project_id = if p.is_global {
            None
        } else {
            p.project_id
                .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
        };

        let mut memory = Memory::new(p.content.clone(), p.memory_type).with_tags(p.tags.clone());

        if let Some(pid) = project_id {
            memory = memory.with_project(pid);
        }

        if let Some(ref s) = p.source {
            if let Ok(source) = s.parse() {
                memory = memory.with_source(source);
            }
        }
        if let Some(ref s) = p.scope {
            if let Ok(scope) = s.parse() {
                memory = memory.with_scope(scope);
            }
        }

        memory
    }

    /// 应用去重决策（T6 拆分）：
    /// - Skip → `skipped` 响应；
    /// - Update → 窗口内合并旧记录（`updated` 响应），窗口外仅标注 superseded
    ///   后返回 None 走正常新增；
    /// - Add / 无目标 → None 走正常新增。
    /// 窗口内合并失败、目标丢失同样回退 None（按新增处理）。
    pub(super) async fn apply_dedup_decision(
        &self,
        p: &mr_protocol::AddParams,
        new_memory: &Memory,
        decision: DedupDecision,
        id: u64,
    ) -> Option<JsonRpcResponse> {
        match decision.action {
            DedupAction::Skip => {
                tracing::info!(
                    "Add skipped: duplicate of {:?} (score={:.3})",
                    decision.candidates.first().map(|c| c.memory_id),
                    decision.candidates.first().map(|c| c.score).unwrap_or(0.0)
                );
                Some(JsonRpcResponse::success(
                    ResponseResult::Memory(MemoryResult {
                        memory: new_memory.clone(),
                        action: "skipped".to_string(),
                        duplicates: decision.candidates,
                    }),
                    id,
                ))
            }
            DedupAction::Update(target_id) => {
                let old = match self.storage.get(&target_id).await {
                    Ok(Some(old)) => old,
                    Ok(None) => {
                        tracing::warn!(
                            "Add dedup target {:?} not found, falling back to add",
                            target_id
                        );
                        return None;
                    }
                    Err(_) => {
                        return None;
                    }
                };

                // 更新窗口（附则 A.1）：仅窗口内允许原地合并；
                // 窗口外的"修正"不覆盖历史，改为新增 + 旧记录标记 superseded。
                let window = self.dedup_config.update_window_mins;
                let age_mins = (chrono::Utc::now() - old.created_at).num_minutes();
                if window != 0 && age_mins > window as i64 {
                    self.mark_superseded(target_id, &new_memory.id).await;
                    tracing::info!(
                        "Add: target {:?} outside update window ({}m > {}m); adding new + marking superseded",
                        target_id,
                        age_mins,
                        window
                    );
                    return None;
                }

                match self.merge_update_in_window(p, &old).await {
                    Ok(updated) => {
                        tracing::info!("Add updated existing memory {:?}", target_id);
                        Some(JsonRpcResponse::success(
                            ResponseResult::Memory(MemoryResult {
                                memory: updated,
                                action: "updated".to_string(),
                                duplicates: decision.candidates,
                            }),
                            id,
                        ))
                    }
                    Err(e) => {
                        tracing::warn!("Add dedup update failed ({}), falling back to add", e);
                        None
                    }
                }
            }
            DedupAction::Add => None,
        }
    }

    /// 窗口内去重合并（T6 拆分）：以新内容覆写旧记录并并入新标签，
    /// 完成后重建向量/全文索引并归档。
    pub(super) async fn merge_update_in_window(
        &self,
        p: &mr_protocol::AddParams,
        old: &Memory,
    ) -> anyhow::Result<Memory> {
        let mut updated = old.clone();
        updated.content = p.content.clone();
        for t in &p.tags {
            if !updated.tags.contains(t) {
                updated.tags.push(t.clone());
            }
        }
        updated.last_accessed = chrono::Utc::now();
        updated.access_count += 1;

        self.storage.update(&updated).await?;

        // 重建向量索引：先删旧向量再写新向量
        let _ = self.hybrid_store.remove(&updated.id).await;
        if let Ok(embed) = self.embedder.embed(&updated.content) {
            let payload = self.build_payload(&updated);
            let _ = self
                .hybrid_store
                .add(&updated.id, &embed, &updated.content, payload)
                .await;
        }
        self.archive_manager.archive(&updated).await;

        Ok(updated)
    }

    /// 窗口外修正（T6 拆分）：仅给旧记录打 superseded 标记，不覆盖历史内容。
    pub(super) async fn mark_superseded(&self, target_id: uuid::Uuid, superseder: &uuid::Uuid) {
        if let Ok(Some(mut old)) = self.storage.get(&target_id).await {
            if !old.tags.iter().any(|t| t == "superseded") {
                old.tags.push("superseded".to_string());
            }
            old.metadata
                .insert("superseded_by".to_string(), superseder.to_string());
            old.metadata
                .insert("superseded_at".to_string(), chrono::Utc::now().to_rfc3339());
            if let Err(e) = self.storage.update(&old).await {
                tracing::warn!("Failed to mark {:?} superseded: {}", target_id, e);
            }
        }
    }

    /// 新增路径（T6 拆分）：save + 归档 + 嵌入/全文索引重建。
    ///
    /// 索引重建失败仅告警不回滚（记忆主体已落库，索引可通过重生成补齐）。
    pub(super) async fn save_and_index(&self, memory: &Memory, content: &str) -> anyhow::Result<()> {
        self.storage.save(memory).await?;
        self.archive_manager.archive(memory).await;

        match self.embedder.embed(content) {
            Ok(embed) => {
                let payload = self.build_payload(memory);
                self.hybrid_store
                    .add(&memory.id, &embed, content, payload)
                    .await
                    .ok();
            }
            Err(e) => {
                tracing::warn!("Failed to generate embedding: {}", e);
            }
        }

        Ok(())
    }

    /// 构造向量索引载荷。
    pub(super) fn build_payload(&self, memory: &Memory) -> VectorPayload {
        VectorPayload {
            project_id: memory.project_id,
            memory_type: memory.memory_type.to_string(),
            tags: memory.tags.clone(),
            content_preview: memory.content.chars().take(200).collect(),
            importance: memory.importance,
            chunk_group_id: memory.chunk_group_id,
            chunk_index: memory.chunk_index,
            chunk_total: memory.chunk_total,
        }
    }

    /// 写入去重检测：检索相似记忆并做出决策。
    ///
    /// 使用向量检索（余弦相似度）查找相似候选，过滤低于 `update_threshold` 的命中，
    /// 交由决策器（规则版/LLM 版）判断动作。
    /// 注意：此处直接用向量分数而非混合检索——混合检索的融合归一化分数
    /// 会稀释原始相似度，不适合阈值判断。
    pub(super) async fn check_dedup(&self, content: &str) -> Option<DedupDecision> {
        let decider = self.dedup_decider.as_ref()?;
        if !self.dedup_config.enabled {
            return None;
        }

        let embed = self.embedder.embed(content).ok()?;
        let filter = SearchFilter {
            project_id: None,
            include_global: true,
            memory_type: None,
            min_score: 0.0,
            ..Default::default()
        };
        let hits = self
            .vector_store
            .search(&embed, filter, self.dedup_config.top_k.max(1))
            .await
            .ok()?;

        let candidates: Vec<SimilarMemory> = hits
            .into_iter()
            .filter(|h| h.score >= self.dedup_config.update_threshold)
            .map(|h| SimilarMemory {
                memory_id: h.memory_id,
                content: h.payload.content_preview,
                score: h.score,
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        Some(decider.decide(content, &candidates).await)
    }

}
