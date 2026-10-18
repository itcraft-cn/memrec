//! Dream 整合主流程 `DreamProcessor::process_inner` 及其阶段拆分。
//!
//! 原 `process_inner` 约 437 行，现拆为「候选筛选 / 聚类 / Stage F 融合 /
//! Stage N 成网」四个阶段方法，`process_inner` 仅作编排，行为保持不变。

use super::helpers::*;
use super::*;

/// Stage F 融合阶段的产出集合。
pub(super) struct FusionStage {
    /// 新写入（或预演统计）的产出记忆
    pub(super) produced: Vec<Memory>,
    /// 全部产出记忆 id（事实条 + 主题摘要）
    pub(super) integrated_ids: Vec<Uuid>,
    /// 主题摘要记忆 id
    pub(super) summary_ids: Vec<Uuid>,
    /// 事实条对其源记忆的引用对 (事实条 id, 源记忆 id)
    pub(super) references: Vec<(Uuid, Uuid)>,
    /// 矛盾对 (源 a, 源 b, 原因)
    pub(super) contradictions: Vec<(Uuid, Uuid, String)>,
    /// 全部摘要文本
    pub(super) summary_texts: Vec<String>,
}

impl DreamProcessor {
    pub(super) async fn process_inner(
        &self,
        run: &mut DreamRun,
        dry_run: bool,
    ) -> Result<DreamResult, DreamError> {
        // 先筛选符合条件的旧记忆；数量不足时直接返回，不执行任何阶段。
        let cutoff = Utc::now() - chrono::Duration::hours(self.config.max_age_hours as i64);
        let cutoff_str = cutoff.to_rfc3339();
        let mut candidates = self.select_candidates(cutoff, dry_run).await?;

        // 门禁先于截断：以“可整合候选总数”判定是否达到最小门槛，避免
        // integration_max_memories < min_memories 时被截断后误报“记忆不足”。
        if candidates.len() < self.config.min_memories {
            warn!(
                "Not enough memories for Dream integration: {} < {}",
                candidates.len(),
                self.config.min_memories
            );
            run.status = DreamRunStatus::NoOp;
            return Ok(DreamResult::empty(
                "Not enough old memories for Dream integration".to_string(),
                dry_run,
            ));
        }

        candidates.truncate(self.config.integration_max_memories);
        run.candidate_count = candidates.len();

        info!(
            "Dream processing {} memories older than {}",
            candidates.len(),
            cutoff_str
        );

        // ---- Stage F 融合：零 LLM 聚类 + 分簇提炼 + 写入产出 ----
        let candidate_emb = self.collect_embeddings(&candidates).await;
        let clusters = self.cluster(&candidates, &candidate_emb);
        run.cluster_count = clusters.len();
        let fusion = self.stage_fuse(run, &candidates, &clusters, dry_run).await?;

        let summary_joined = fusion.summary_texts.join("\n\n");
        let summary_clues = extract_summary_clues(&summary_joined);
        let created_memory_id = fusion
            .summary_ids
            .last()
            .copied()
            .or_else(|| fusion.integrated_ids.first().copied());

        // 预演：到此为止，不删源、不建边、不执行阶段。
        if dry_run {
            let metrics = self.compute_metrics(
                &candidates,
                &fusion.produced,
                run.cluster_count,
                &fusion.contradictions,
                0,
                0,
                0.0,
            );
            run.metrics = metrics.clone();
            run.finish(DreamRunStatus::Success);
            return Ok(DreamResult {
                run_id: Some(run.run_id),
                integrated_count: candidates.len(),
                created_memory_id,
                summary: summary_joined,
                summary_clues,
                phase_results: Vec::new(),
                cluster_count: run.cluster_count,
                contradiction_count: fusion.contradictions.len(),
                edge_count: 0,
                propagation_count: 0,
                dry_run: true,
                metrics,
            });
        }

        // 软删源记忆
        for memory in &candidates {
            self.storage
                .delete(&memory.id)
                .await
                .map_err(DreamError::from)?;
            run.deleted_source_ids.push(memory.id);
        }

        // ---- Stage N 成网：零 LLM 边 + 传播增量 ----
        let (edge_count, propagation_count) = self
            .stage_network(run, &candidates, &candidate_emb, &fusion)
            .await;

        // 阶段 1-4
        let phase_results = self.run_phases().await;
        run.phase_results = phase_results.iter().map(|p| p.to_record()).collect();

        let phase_success_rate = if phase_results.is_empty() {
            1.0
        } else {
            phase_results.iter().filter(|p| p.success).count() as f32 / phase_results.len() as f32
        };
        let metrics = self.compute_metrics(
            &candidates,
            &fusion.produced,
            run.cluster_count,
            &fusion.contradictions,
            edge_count,
            propagation_count,
            phase_success_rate,
        );
        run.metrics = metrics.clone();

        run.finish(DreamRunStatus::Success);

        info!(
            "Dream completed: {} memories integrated, {} edges, {} propagations",
            candidates.len(),
            edge_count,
            propagation_count
        );

        Ok(DreamResult {
            run_id: Some(run.run_id),
            integrated_count: candidates.len(),
            created_memory_id,
            summary: summary_joined,
            summary_clues,
            phase_results,
            cluster_count: run.cluster_count,
            contradiction_count: fusion.contradictions.len(),
            edge_count,
            propagation_count,
            dry_run: false,
            metrics,
        })
    }

    /// 候选筛选：时间区间下推取数 → 高价值/过短过滤 → 已整合源幂等补删 → 按重要性升序。
    async fn select_candidates(
        &self,
        cutoff: DateTime<Utc>,
        dry_run: bool,
    ) -> Result<Vec<Memory>, DreamError> {
        // P0/T1: 候选必然早于 cutoff，用时间区间下推替代全量加载。
        let all = self
            .storage
            .list_created_between(0, cutoff.timestamp(), usize::MAX)
            .await
            .map_err(DreamError::from)?;

        // P4: 排除高价值记忆（importance ≥ 阈值、命中保留标签、保留类型），
        // 同时过滤过短内容（测试/占位垃圾记录）。
        let min_content_len = self.config.integration_min_content_len;
        let mut candidates: Vec<Memory> = all
            .into_iter()
            .filter(|m| !m.is_deleted && m.created_at < cutoff)
            .filter(|m| {
                m.importance < self.config.integration_preserve_importance
                    && !m
                        .tags
                        .iter()
                        .any(|t| self.config.integration_preserve_tags.contains(t))
                    && !self
                        .config
                        .integration_preserve_types
                        .iter()
                        .any(|t| t == &m.memory_type.to_string())
                    && m.content.trim().chars().count() >= min_content_len
            })
            .collect();

        // P5: 排除已被整合的源记忆；上次崩溃残留的已整合源记忆补删，保证幂等。
        let seen = self.collect_integrated_source_ids().await?;
        if !seen.is_empty() {
            let mut leftover: Vec<Uuid> = Vec::new();
            candidates.retain(|m| {
                if seen.contains(&m.id) {
                    leftover.push(m.id);
                    false
                } else {
                    true
                }
            });
            if !dry_run {
                for id in leftover {
                    if let Err(e) = self.storage.delete(&id).await {
                        warn!("Failed to delete leftover integrated memory {}: {}", id, e);
                    }
                }
            }
        }

        // P1/P3: 按重要性升序（优先整合低价值记忆），截取单次整合上限。
        candidates.sort_by(|a, b| {
            a.importance
                .partial_cmp(&b.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(candidates)
    }

    /// Stage F 聚类：启用时按向量聚类，否则整批视为单簇。
    fn cluster(
        &self,
        candidates: &[Memory],
        candidate_emb: &HashMap<Uuid, Vec<f32>>,
    ) -> Vec<Vec<usize>> {
        if self.config.fusion_cluster_enabled {
            cluster_candidates(
                candidates,
                candidate_emb,
                self.config.fusion_cluster_threshold,
                self.config.fusion_cluster_max_size,
                self.config.fusion_max_clusters,
            )
        } else {
            vec![(0..candidates.len()).collect()]
        }
    }

    /// Stage F 融合：分簇提炼（LLM 故障在写入前即中止，保证一致性）+ 写入产出。
    async fn stage_fuse(
        &self,
        run: &mut DreamRun,
        candidates: &[Memory],
        clusters: &[Vec<usize>],
        dry_run: bool,
    ) -> Result<FusionStage, DreamError> {
        let use_llm = !dry_run || self.config.dry_run_with_llm;
        let mut extractions: Vec<(Vec<usize>, crate::dream::fusion::ClusterExtraction)> =
            Vec::with_capacity(clusters.len());
        for cluster in clusters {
            if cluster.is_empty() {
                continue;
            }
            let cluster_mems: Vec<Memory> =
                cluster.iter().map(|&i| candidates[i].clone()).collect();
            match extract_cluster(self.llm.as_ref(), &cluster_mems, use_llm).await {
                Ok(x) => extractions.push((cluster.clone(), x)),
                Err(e) => return Err(DreamError::Llm(e)),
            }
        }

        // 写入产出（dry-run 时只统计不落库）。
        let mut integrated_ids: Vec<Uuid> = Vec::new();
        let mut summary_ids: Vec<Uuid> = Vec::new();
        let mut produced: Vec<Memory> = Vec::new();
        let mut references: Vec<(Uuid, Uuid)> = Vec::new();
        let mut contradictions: Vec<(Uuid, Uuid, String)> = Vec::new();
        let mut summary_texts: Vec<String> = Vec::new();

        let mem_type = parse_memory_type(&self.config.integration_type);
        // 上游去重：跨簇累积已保留事实条的嵌入，用于近重复判定。
        let mut kept_fact_embs: Vec<Vec<f32>> = Vec::new();
        let mut dedup_dropped = 0usize;

        for (cluster, extraction) in &extractions {
            let source_ids: Vec<String> = cluster
                .iter()
                .map(|&i| candidates[i].id.to_string())
                .collect();
            let mut base_meta: HashMap<String, String> = HashMap::new();
            base_meta.insert(
                "source_ids".to_string(),
                serde_json::to_string(&source_ids).unwrap_or_default(),
            );
            base_meta.insert("dream_run_id".to_string(), run.run_id.to_string());
            base_meta.insert("integrated_count".to_string(), cluster.len().to_string());

            // 事实条
            if self.config.fusion_emit_facts {
                for fact in &extraction.facts {
                    let mut meta = base_meta.clone();
                    if !fact.source_hints.is_empty() {
                        meta.insert(
                            "source_hints".to_string(),
                            serde_json::to_string(&fact.source_hints).unwrap_or_default(),
                        );
                    }
                    let mut tags = self.config.integration_tags.clone();
                    tags.push(TAG_INTEGRATED.to_string());
                    tags.push(TAG_FACT.to_string());
                    let mut m = build_produced_memory(fact.text.clone(), mem_type, tags, meta, 0.7);

                    // 上游去重：事实条与已保留事实条近重复（余弦 >= 阈值）则丢弃，
                    // 仅保留最先产出的一条，避免梦产出冗余记忆与"重复当关系"的噪声边。
                    if self.config.fusion_dedup_enabled {
                        if let Some(emb) = self
                            .embedder
                            .as_ref()
                            .and_then(|e| e.embed(&m.content).ok())
                        {
                            if kept_fact_embs.iter().any(|k| {
                                cosine_similarity(k, &emb) >= self.config.fusion_dedup_threshold
                            }) {
                                dedup_dropped += 1;
                                continue;
                            }
                            m.embedding = Some(emb.clone());
                            kept_fact_embs.push(emb);
                        }
                    }

                    let mid = m.id;
                    if !dry_run {
                        self.storage.add(&m).await.map_err(DreamError::from)?;
                    }
                    integrated_ids.push(mid);
                    produced.push(m);
                    // 事实条引用本簇全部源
                    for &i in cluster {
                        references.push((mid, candidates[i].id));
                    }
                }
            }

            // 主题摘要
            if self.config.fusion_emit_summary {
                let content = clean_summary(&extraction.summary);
                let mut tags = self.config.integration_tags.clone();
                tags.push(TAG_INTEGRATED.to_string());
                tags.push(TAG_SUMMARY.to_string());
                let m =
                    build_produced_memory(content.clone(), mem_type, tags, base_meta.clone(), 0.8);
                let mid = m.id;
                if !dry_run {
                    self.storage.add(&m).await.map_err(DreamError::from)?;
                }
                summary_ids.push(mid);
                integrated_ids.push(mid);
                summary_texts.push(content);
                produced.push(m);
            }

            // 矛盾对
            if self.config.fusion_detect_contradictions {
                for c in &extraction.contradictions {
                    if c.a < cluster.len() && c.b < cluster.len() && c.a != c.b {
                        contradictions.push((
                            candidates[cluster[c.a]].id,
                            candidates[cluster[c.b]].id,
                            c.reason.clone(),
                        ));
                    }
                }
            }
        }

        if dedup_dropped > 0 {
            tracing::info!(
                dropped = dedup_dropped,
                threshold = self.config.fusion_dedup_threshold,
                "Dream Stage F 上游去重：丢弃近重复事实条"
            );
        }

        run.integrated_memory_ids = integrated_ids.clone();
        run.summary_memory_id = summary_ids.last().copied();
        run.contradiction_count = contradictions.len();

        Ok(FusionStage {
            produced,
            integrated_ids,
            summary_ids,
            references,
            contradictions,
            summary_texts,
        })
    }

}
