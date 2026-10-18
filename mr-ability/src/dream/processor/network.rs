//! Dream Stage N 成网阶段（零 LLM 边、可选语义边、传播增量）。

use super::helpers::*;
use super::pipeline::FusionStage;
use super::*;

impl DreamProcessor {
    /// Stage N 成网：零 LLM 边 + 可选语义边 + 传播增量，返回 (边数, 传播增量数)。
    pub(super) async fn stage_network(
        &self,
        run: &mut DreamRun,
        candidates: &[Memory],
        candidate_emb: &HashMap<Uuid, Vec<f32>>,
        fusion: &FusionStage,
    ) -> (usize, usize) {
        let produced = &fusion.produced;
        let references = &fusion.references;
        let contradictions = &fusion.contradictions;

        let mut edge_count = 0usize;
        if self.config.networking_enabled {
            if let Some(edge_store) = &self.edge_store {
                let produced_emb = self.collect_embeddings(produced).await;
                let mut pool_emb = candidate_emb.clone();
                for (k, v) in &produced_emb {
                    pool_emb.insert(*k, v.clone());
                }
                let mut pool = candidates.to_vec();
                pool.extend(produced.iter().cloned());
                let builder = EdgeBuilder::new(self.config.clone());
                match builder
                    .build(
                        produced,
                        &produced_emb,
                        &pool,
                        &pool_emb,
                        references,
                        contradictions,
                        edge_store,
                    )
                    .await
                {
                    Ok(stats) => {
                        edge_count = stats.total;
                        run.edge_count = stats.total;
                        run.edge_counts_by_type = stats.by_type;
                    }
                    Err(e) => warn!(target: "dream", "Edge build failed: {}", e),
                }

                // Stage N 语义边（P2-8，默认关）：LLM 判断 CausedBy/Evolves/References
                if self.config.networking_semantic_enabled {
                    if let Some(llm) = &self.llm {
                        let gen = crate::edge::LlmEdgeGenerator::new(llm.clone(), 3, 0.6);
                        let max_candidates = self.config.networking_semantic_max_candidates;
                        for m in produced {
                            // 语义边候选池限幅：按向量相似度取最相近的 top-N，排除自身与
                            // 近重复（近重复应走上游去重，而非建立关系边），避免把全量
                            // 候选+产出整体入参导致成本随规模线性膨胀。
                            let m_emb = produced_emb.get(&m.id);
                            let mut scored: Vec<(&Memory, f32)> = Vec::new();
                            for cand in &pool {
                                if cand.id == m.id {
                                    continue;
                                }
                                let s = match (m_emb, pool_emb.get(&cand.id)) {
                                    (Some(a), Some(b)) => cosine_similarity(a, b),
                                    _ => 0.0,
                                };
                                if s >= self.config.networking_semantic_dup_threshold {
                                    continue;
                                }
                                scored.push((cand, s));
                            }
                            scored.sort_by(|a, b| {
                                b.1.partial_cmp(&a.1)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                                    .then_with(|| a.0.id.cmp(&b.0.id))
                            });
                            let candidates_limited: Vec<Memory> = if max_candidates == 0 {
                                Vec::new()
                            } else {
                                scored
                                    .iter()
                                    .take(max_candidates)
                                    .map(|(c, _)| (*c).clone())
                                    .collect()
                            };

                            match gen
                                .generate(m, &candidates_limited, edge_store.as_ref())
                                .await
                            {
                                Ok(edges) => {
                                    for e in edges {
                                        if let Err(err) = edge_store.save(&e).await {
                                            warn!(target: "dream", "Save semantic edge failed: {}", err);
                                        } else {
                                            edge_count += 1;
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(target: "dream", "Semantic edge generation failed: {}", e)
                                }
                            }
                        }
                        run.edge_count = edge_count;
                    }
                }
            }
        }

        // 传播增量
        let mut propagation_count = 0usize;
        if self.config.networking_propagation_enabled {
            if let Some(engine) = &self.propagation {
                for m in produced {
                    match engine.propagate(m).await {
                        Ok(r) => propagation_count += r.delta_count,
                        Err(e) => warn!(target: "dream", "Propagation failed for {}: {}", m.id, e),
                    }
                }
            }
        }
        run.propagation_count = propagation_count;

        (edge_count, propagation_count)
    }
}
