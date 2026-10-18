//! # 检索处理器（混合检索与分层检索）
//!
//! [`Router`] 的 SearchMemory / TieredSearch 及其结果构建方法。

use super::*;

impl Router {
    /// 处理 SearchMemory 请求：混合搜索（向量 + 全文）。
    ///
    /// 流程：生成查询嵌入 → 混合搜索（KNN + BM25）→ MMR重排 → 补充记忆元数据 → 返回结果。
    /// 返回结果包含嵌入耗时和搜索耗时。
    pub(super) async fn handle_search_memory(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::SearchMemory(p)) => {
                let start = Instant::now();

                let embedding = match self.embedder.embed(&p.query) {
                    Ok(e) => e,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32002,
                                message: format!("Embedding error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };
                let embed_time = start.elapsed().as_millis() as u64;

                let project_id = if p.cross_project || p.global_only {
                    // T4: 全局语义统一为 None；global_only 由 SearchFilter.global_only 承载
                    None
                } else if p.project_only {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                } else {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                };

                let include_global = !p.project_only && !p.cross_project && !p.global_only;

                let filter = SearchFilter {
                    project_id,
                    include_global,
                    global_only: p.global_only,
                    memory_type: p.memory_type.map(|t| t.to_string()),
                    min_score: p.min_score,
                };

                let search_start = Instant::now();
                let req = HybridSearchRequest {
                    query: p.query.clone(),
                    query_embedding: embedding,
                    filter,
                    top_k: p.top_k,
                    hybrid_alpha: p.hybrid_alpha as f32,
                    mmr_lambda: p.mmr_lambda as f32,
                    mmr_enabled: p.mmr_enabled,
                };

                let result = match self.hybrid_store.search(req).await {
                    Ok(r) => r,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32003,
                                message: format!("Search error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };
                let search_time = search_start.elapsed().as_millis() as u64;

                let mut results: Vec<SearchHit> = vec![];
                // 批量回表，消除逐条 storage.get 的 N+1（M-1）。取数失败时退化为缺省元数据。
                let hit_ids: Vec<Uuid> = result.hits.iter().map(|h| h.memory_id).collect();
                let fetched = self
                    .storage
                    .get_many(&hit_ids)
                    .await
                    .unwrap_or_else(|_| vec![None; hit_ids.len()]);
                for (h, memory) in result.hits.iter().zip(fetched) {
                    let memory_type = memory
                        .as_ref()
                        .map(|m| m.memory_type)
                        .unwrap_or(MemoryType::Conversation);
                    let created_at = memory.map(|m| m.created_at).unwrap_or_default();
                    results.push(SearchHit {
                        memory_id: h.memory_id,
                        score: h.score,
                        memory_type,
                        content_preview: h.payload.content_preview.clone(),
                        project_id: h.payload.project_id,
                        tags: h.payload.tags.clone(),
                        is_chunked: h.payload.chunk_group_id.is_some(),
                        chunk_group_id: h.payload.chunk_group_id,
                        chunk_index: h.payload.chunk_index,
                        chunk_total: h.payload.chunk_total,
                        created_at,
                        edge_hint: None,
                    });
                }

                // 图扩散（P1-6）：seed 命中经图扩散补充召回。
                // 图命中分 = seed_score × graph_score × retrieval_graph_weight；
                // 经图到达的条目带 EdgeHint 标注；软删记忆不返回（仅用于连通）。
                if self.retrieval_graph.enabled {
                    tracing::debug!(
                        target: "recall",
                        "retrieval graph enabled: edge_store={}",
                        self.edge_store.is_some()
                    );
                    if let Some(edge_store) = &self.edge_store {
                        let seed_ids: Vec<Uuid> = result.hits.iter().map(|h| h.memory_id).collect();
                        let seed_score: HashMap<Uuid, f32> =
                            result.hits.iter().map(|h| (h.memory_id, h.score)).collect();
                        let params = mr_ability::storage::GraphTraversalParams {
                            // traverse 的 max_depth 含种子层，故“跳数”= depth 时取 depth+1
                            max_depth: self.retrieval_graph.depth + 1,
                            decay: self.retrieval_graph.decay,
                            min_score: 0.0,
                            max_results: p.top_k.max(1) * 3,
                            edge_types: Some(vec![
                                mr_common::EdgeType::Similar,
                                mr_common::EdgeType::References,
                                mr_common::EdgeType::CausedBy,
                                mr_common::EdgeType::Evolves,
                            ]),
                        };
                        if let Ok(graph_hits) = edge_store.traverse(&seed_ids, params).await {
                            tracing::debug!(
                                target: "recall",
                                "graph traversal: seeds={} graph_hits={}",
                                seed_ids.len(),
                                graph_hits.len()
                            );
                            let existing: HashSet<Uuid> =
                                results.iter().map(|r| r.memory_id).collect();
                            // 批量预取图命中记忆，消除循环内逐条回表（M-2）。
                            let graph_ids: Vec<Uuid> =
                                graph_hits.iter().map(|gh| gh.memory_id).collect();
                            let graph_fetched = self
                                .storage
                                .get_many(&graph_ids)
                                .await
                                .unwrap_or_else(|_| vec![None; graph_ids.len()]);
                            let mut graph_mem: HashMap<Uuid, Memory> = graph_ids
                                .iter()
                                .copied()
                                .zip(graph_fetched)
                                .filter_map(|(id, m)| m.map(|m| (id, m)))
                                .collect();
                            let mut pushed = 0usize;
                            for gh in graph_hits {
                                if existing.contains(&gh.memory_id) {
                                    tracing::debug!(target:"recall","gh {} skip: already in seeds", gh.memory_id);
                                    continue;
                                }
                                let seed = gh.path.first().copied().unwrap_or(gh.memory_id);
                                let base = seed_score.get(&seed).copied().unwrap_or(0.0);
                                let score = base * gh.score * self.retrieval_graph.weight;
                                if score <= 0.0 {
                                    tracing::debug!(target:"recall","gh {} skip: score<=0", gh.memory_id);
                                    continue;
                                }
                                let memory = match graph_mem.remove(&gh.memory_id) {
                                    Some(m) => m,
                                    None => {
                                        tracing::debug!(target:"recall","gh {} skip: memory missing", gh.memory_id);
                                        continue;
                                    }
                                };
                                if memory.is_deleted {
                                    tracing::debug!(target:"recall","gh {} skip: deleted", gh.memory_id);
                                    continue;
                                }
                                pushed += 1;
                                let edge_type = gh
                                    .path_types
                                    .last()
                                    .copied()
                                    .unwrap_or(mr_common::EdgeType::Similar);
                                results.push(SearchHit {
                                    memory_id: memory.id,
                                    score,
                                    memory_type: memory.memory_type,
                                    content_preview: memory
                                        .content
                                        .chars()
                                        .take(200)
                                        .collect::<String>(),
                                    project_id: memory.project_id,
                                    tags: memory.tags.clone(),
                                    is_chunked: memory.chunk_group_id.is_some(),
                                    chunk_group_id: memory.chunk_group_id,
                                    chunk_index: memory.chunk_index,
                                    chunk_total: memory.chunk_total,
                                    created_at: memory.created_at,
                                    edge_hint: Some(mr_common::EdgeHint {
                                        edge_type,
                                        target_summary: gh
                                            .path
                                            .iter()
                                            .map(|u| u.to_string())
                                            .collect::<Vec<_>>()
                                            .join(" -> "),
                                        weight: gh.score,
                                    }),
                                });
                            }
                            tracing::debug!(target:"recall","graph hits pushed={}", pushed);
                        }
                    }
                }

                // 统一按分排序（含种子与图命中）
                results.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.memory_id.cmp(&b.memory_id))
                });

                // 图命中保留名额：图命中分 = seed_score × graph_score × weight，恒低于
                // 其种子分；若仅按分合并后截断，图命中必被种子填满的 top_k 挤出，图扩散
                // 等同失效。故为图命中预留 quota×top_k 个名额，使其能进入最终结果。
                let top_k = p.top_k.max(1);
                let graph_slots =
                    ((top_k as f32) * self.retrieval_graph.quota.clamp(0.0, 1.0)).ceil() as usize;
                if graph_slots > 0 {
                    let (graph_hits, seed_hits): (Vec<SearchHit>, Vec<SearchHit>) =
                        results.into_iter().partition(|r| r.edge_hint.is_some());
                    if !graph_hits.is_empty() {
                        let g = graph_slots.min(graph_hits.len());
                        let s = top_k.saturating_sub(g);
                        let mut merged: Vec<SearchHit> = seed_hits.into_iter().take(s).collect();
                        merged.extend(graph_hits.into_iter().take(g));
                        // 最终仍按分展示排序
                        merged.sort_by(|a, b| {
                            b.score
                                .partial_cmp(&a.score)
                                .unwrap_or(std::cmp::Ordering::Equal)
                                .then_with(|| a.memory_id.cmp(&b.memory_id))
                        });
                        results = merged;
                    } else {
                        results = seed_hits;
                    }
                }
                results.truncate(top_k);

                let total = results.len();

                JsonRpcResponse::success(
                    ResponseResult::SemanticSearchResult(SemanticSearchResult {
                        results,
                        total,
                        query_embedding_time_ms: embed_time,
                        search_time_ms: search_time,
                    }),
                    id,
                )
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for SearchMemory".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }

    pub(super) async fn handle_tiered_search(
        &self,
        params: Option<RequestParams>,
        id: u64,
    ) -> JsonRpcResponse {
        match params {
            Some(RequestParams::TieredSearch(p)) => {
                let start = Instant::now();

                let embedding = match self.embedder.embed(&p.query) {
                    Ok(e) => e,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32002,
                                message: format!("Embedding error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };
                let _embed_time = start.elapsed().as_millis() as u64;

                let project_id = if p.cross_project || p.global_only {
                    // T4: 全局语义统一为 None；global_only 由 SearchFilter.global_only 承载
                    None
                } else if p.project_only {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                } else {
                    p.project_id
                        .or_else(|| detect_project_id(p.working_dir.as_deref()).ok())
                };

                let include_global = !p.project_only && !p.cross_project && !p.global_only;

                let filter = SearchFilter {
                    project_id,
                    include_global,
                    global_only: p.global_only,
                    memory_type: p.memory_type.map(|t| t.to_string()),
                    min_score: p.min_score,
                };

                let req = HybridSearchRequest {
                    query: p.query.clone(),
                    query_embedding: embedding,
                    filter,
                    top_k: p.top_k,
                    hybrid_alpha: 0.5,
                    mmr_lambda: 0.5,
                    mmr_enabled: true,
                };

                let result = match self.hybrid_store.search(req).await {
                    Ok(r) => r,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32003,
                                message: format!("Search error: {}", e),
                                data: None,
                            },
                            id,
                        )
                    }
                };

                let memory_ids: Vec<Uuid> = result.hits.iter().map(|h| h.memory_id).collect();
                let scores: Vec<f32> = result.hits.iter().map(|h| h.score).collect();

                let thresholds = p
                    .tier_thresholds
                    .as_ref()
                    .map(|t| TierThresholds {
                        full: t.full,
                        truncated: t.truncated,
                        summary: t.summary,
                    })
                    .unwrap_or_default();

                let assigner = TierAssigner::new(thresholds);
                let budget = mr_ability::tiered::BudgetController::new(p.max_total_tokens);

                match self
                    .build_tiered_result(memory_ids, scores, assigner, budget)
                    .await
                {
                    Ok(tiered_result) => JsonRpcResponse::success(
                        ResponseResult::TieredSearchResult(tiered_result),
                        id,
                    ),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32005,
                            message: format!("Tiered search error: {}", e),
                            data: None,
                        },
                        id,
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32602,
                    message: "Invalid params for TieredSearch".to_string(),
                    data: None,
                },
                id,
            ),
        }
    }


    pub(super) async fn build_tiered_result(
        &self,
        memory_ids: Vec<Uuid>,
        scores: Vec<f32>,
        assigner: TierAssigner,
        budget: mr_ability::tiered::BudgetController,
    ) -> Result<mr_common::TieredSearchResult> {
        use mr_common::TieredMemory;

        let mut results = Vec::with_capacity(memory_ids.len());

        for (memory_id, score) in memory_ids.iter().zip(scores.iter()) {
            if let Some(memory) = self.storage.get(memory_id).await? {
                let tier = assigner.assign(*score);
                let (content, token_count) = Self::generate_tier_content(&memory.content, tier);

                results.push(TieredMemory {
                    memory_id: *memory_id,
                    tier,
                    content,
                    score: *score,
                    token_count,
                    facet_themes: vec![],
                    edge_hints: vec![],
                });
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_tokens = budget.check_and_downgrade(&mut results);

        Ok(mr_common::TieredSearchResult {
            total_results: results.len(),
            total_tokens,
            budget: budget.max_tokens(),
            results,
        })
    }

    pub(super) fn generate_tier_content(content: &str, tier: RepresentationTier) -> (String, u32) {
        let tokens = mr_ability::tiered::BudgetController::estimate_token_count(content);

        match tier {
            RepresentationTier::Full => (content.to_string(), tokens),
            RepresentationTier::Truncated => {
                let limit = 500;
                let truncated = if content.chars().count() > limit {
                    let chars: Vec<char> = content.chars().take(limit).collect();
                    let truncated: String = chars.into_iter().collect();
                    format!("{}...[truncated]", truncated)
                } else {
                    content.to_string()
                };
                let token_count =
                    mr_ability::tiered::BudgetController::estimate_token_count(&truncated);
                (truncated, token_count.min(200))
            }
            RepresentationTier::Summary => {
                let preview = if content.chars().count() > 100 {
                    let chars: Vec<char> = content.chars().take(100).collect();
                    chars.into_iter().collect()
                } else {
                    content.to_string()
                };
                (format!("Summary: {}", preview), 80)
            }
            RepresentationTier::DenseProxy => {
                let words: Vec<&str> = content.split_whitespace().take(5).collect();
                (format!("[{}]", words.join(", ")), 30)
            }
        }
    }
}
