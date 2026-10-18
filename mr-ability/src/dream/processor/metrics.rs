//! Dream 阶段执行与质量度量。

use super::*;

impl DreamProcessor {
    /// 执行阶段 1-4（交叉项目 / 个人摘要 / 清理 / 向量重生成）。
    pub(super) async fn run_phases(&self) -> Vec<PhaseResult> {
        let mut phase_results = Vec::new();

        if self.config.phase_cross_project {
            let extractor = CrossProjectExtractor::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            phase_results.push(extractor.execute().await);
        }

        if self.config.phase_personal_summary {
            let summarizer = PersonalSummarizer::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
            );
            phase_results.push(summarizer.execute().await);
        }

        if self.config.phase_cleanup {
            let cleaner = MemoryCleaner::new(
                self.storage.clone(),
                self.config.batch_size,
                self.config.batch_interval_ms,
                self.config.cleanup_inactive_days,
                self.config.cleanup_importance_threshold,
            );
            phase_results.push(cleaner.execute().await);
        }

        if self.config.phase_vector_regen {
            if let (Some(vector_store), Some(embedder)) = (&self.vector_store, &self.embedder) {
                let regenerator = VectorRegenerator::new(
                    self.storage.clone(),
                    vector_store.clone(),
                    embedder.clone(),
                    self.config.batch_size,
                    self.config.batch_interval_ms,
                );
                phase_results.push(regenerator.execute().await);
            }
        }

        phase_results
    }

    /// 收集记忆向量（优先级：memory.embedding → vector_store → embedder）。
    pub(super) async fn collect_embeddings(&self, mems: &[Memory]) -> HashMap<Uuid, Vec<f32>> {
        let mut map = HashMap::new();
        for m in mems {
            if let Some(v) = &m.embedding {
                if !v.is_empty() {
                    map.insert(m.id, v.clone());
                    continue;
                }
            }
            if let Some(vs) = &self.vector_store {
                if let Ok(Some(v)) = vs.get(&m.id).await {
                    if !v.is_empty() {
                        map.insert(m.id, v);
                        continue;
                    }
                }
            }
            if let Some(e) = &self.embedder {
                if let Ok(v) = e.embed(&m.content) {
                    if !v.is_empty() {
                        map.insert(m.id, v);
                    }
                }
            }
        }
        map
    }

    /// 计算 DreamRun 质量度量。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn compute_metrics(
        &self,
        sources: &[Memory],
        produced: &[Memory],
        cluster_count: usize,
        contradictions: &[(Uuid, Uuid, String)],
        edge_count: usize,
        propagation_count: usize,
        phase_success_rate: f32,
    ) -> QualityMetrics {
        let input_chars: usize = sources.iter().map(|m| m.content.chars().count()).sum();
        let output_chars: usize = produced.iter().map(|m| m.content.chars().count()).sum();
        let input_text: String = sources
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let output_text: String = produced
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let in_clues = extract_summary_clues(&input_text).total();
        let out_clues = extract_summary_clues(&output_text).total();
        let clue_retention = if in_clues == 0 {
            1.0
        } else {
            (out_clues.min(in_clues) as f32) / (in_clues as f32)
        };
        let compression_ratio = if output_chars == 0 {
            0.0
        } else {
            input_chars as f32 / output_chars as f32
        };
        let avg_cluster_size = if cluster_count == 0 {
            0.0
        } else {
            sources.len() as f32 / cluster_count as f32
        };
        QualityMetrics {
            clue_retention,
            compression_ratio,
            avg_cluster_size,
            contradiction_count: contradictions.len(),
            edge_count,
            propagation_count,
            phase_success_rate,
        }
    }

    /// 收集所有已写入整合记忆（`dream-integrated` 标签）记录的源记忆 id。
    ///
    /// 幂等机制：整合记忆写入时把源记忆 id 列表存入 `metadata.source_ids`，
    /// 下次执行据此排除已整合的源记忆，崩溃残留（写后未删）也可识别补删。
    pub(super) async fn collect_integrated_source_ids(&self) -> Result<HashSet<Uuid>, DreamError> {
        let mut seen = HashSet::new();
        // P0/T2: 空 limit 表示不截断。原硬编码 1000 会在整合记忆超限时漏判源记忆，
        // 破坏幂等补删语义。
        let summaries = self
            .storage
            .list_by_tag(TAG_INTEGRATED, usize::MAX)
            .await
            .map_err(DreamError::from)?;
        for m in summaries {
            if let Some(raw) = m.metadata.get("source_ids") {
                if let Ok(ids) = serde_json::from_str::<Vec<String>>(raw) {
                    for s in ids {
                        if let Ok(id) = Uuid::parse_str(&s) {
                            seen.insert(id);
                        }
                    }
                }
            }
        }
        Ok(seen)
    }
}
