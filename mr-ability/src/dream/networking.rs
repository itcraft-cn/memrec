//! # Stage N —— 成网（Networking）
//!
//! 实现 Dream 两段式管线的第二段：把整合产出与源记忆织入记忆图。
//!
//! ## 边类型与来源
//!
//! | 边 | 方向 | 来源 | 是否需要 LLM |
//! |----|------|------|-------------|
//! | `Similar` | 无向 | 向量近邻（produced ↔ 记忆池） | 否 |
//! | `SameProject` | 无向 | produced 之间同项目 | 否 |
//! | `References` | 有向 | produced → 其来源源记忆 | 否 |
//! | `Contradicts` | 无向 | 簇内矛盾对（Stage F 产出） | 否（判定来自 Stage F） |
//! | `CausedBy`/`Evolves` | 有向 | LLM 语义边（默认关） | 是 |
//!
//! 本模块只负责**零 LLM 部分**（P1-5）；语义边由 `LlmEdgeGenerator` 负责，
//! 其默认关闭（P2-8）。
//!
//! 命名正交：本模块不复用 `Scene`。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use mr_common::{DreamConfig, EdgeType, Memory, MemoryEdge};
use uuid::Uuid;

use super::fusion::cosine;
use crate::storage::EdgeStorage;

/// 成网统计。
#[derive(Debug, Clone, Default)]
pub struct EdgeBuildStats {
    /// 新建边总数
    pub total: usize,
    /// 按类型分布（snake_case 类型名 → 数量）
    pub by_type: HashMap<String, usize>,
}

impl EdgeBuildStats {
    fn record(&mut self, edge_type: EdgeType) {
        self.total += 1;
        *self
            .by_type
            .entry(edge_type.to_string())
            .or_insert(0) += 1;
    }
}

/// 记忆图构建器（零 LLM）。
///
/// 只依赖配置与 [`EdgeStorage`]，不持有 LLM，确保 Stage N 的零 LLM 部分
/// 在无 LLM 环境下也能工作。
pub struct EdgeBuilder {
    config: DreamConfig,
}

impl EdgeBuilder {
    /// 创建构建器。
    pub fn new(config: DreamConfig) -> Self {
        Self { config }
    }

    /// 构建本轮的零 LLM 边。
    ///
    /// # 参数
    ///
    /// - `produced`：本轮 Dream 产出的整合记忆
    /// - `produced_emb`：产出记忆的向量（用于 Similar）
    /// - `pool`：可比对的记忆池（候选源 + 产出）
    /// - `pool_emb`：记忆池向量
    /// - `references`：`(produced_id, source_id)` 引用关系
    /// - `contradictions`：`(a_id, b_id, reason)` 矛盾关系
    /// - `edge_store`：边存储
    ///
    /// # 去重
    ///
    /// 同源同目标同类型（无向边忽略方向）已存在则跳过，避免 Dream 反复运行时
    /// 边无限累积。
    #[allow(clippy::too_many_arguments)]
    pub async fn build(
        &self,
        produced: &[Memory],
        produced_emb: &HashMap<Uuid, Vec<f32>>,
        pool: &[Memory],
        pool_emb: &HashMap<Uuid, Vec<f32>>,
        references: &[(Uuid, Uuid)],
        contradictions: &[(Uuid, Uuid, String)],
        edge_store: &Arc<dyn EdgeStorage>,
    ) -> Result<EdgeBuildStats> {
        let mut stats = EdgeBuildStats::default();
        // 本轮去重集合：无向边用 (min,max)，有向边用 (source,target)
        let mut seen: HashSet<(Uuid, Uuid, EdgeType)> = HashSet::new();

        // 1) Similar：每条产出取 top-k 近邻
        if self.config.networking_similar_topk > 0 {
            let pool_index: HashMap<Uuid, &Memory> = pool.iter().map(|m| (m.id, m)).collect();
            for p in produced {
                let Some(pv) = produced_emb.get(&p.id) else {
                    continue;
                };
                let mut scored: Vec<(f32, Uuid)> = Vec::new();
                for (qid, qv) in pool_emb {
                    if *qid == p.id {
                        continue;
                    }
                    if self.config.networking_similar_threshold > 0.0 {
                        // 必须都在池中才建边
                        if !pool_index.contains_key(qid) {
                            continue;
                        }
                    }
                    let sim = cosine(pv, qv);
                    if sim >= self.config.networking_similar_threshold {
                        scored.push((sim, *qid));
                    }
                }
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                for (sim, qid) in scored.into_iter().take(self.config.networking_similar_topk) {
                    let key = unordered_key(p.id, qid, EdgeType::Similar);
                    if !seen.insert(key) {
                        continue;
                    }
                    if self.edge_exists(edge_store, p.id, qid, EdgeType::Similar).await? {
                        continue;
                    }
                    let edge = MemoryEdge::new(
                        p.id,
                        qid,
                        EdgeType::Similar,
                        sim,
                        "dream:vector-neighbor".to_string(),
                    )
                    .auto_generated();
                    edge_store.save(&edge).await?;
                    stats.record(EdgeType::Similar);
                }
            }
        }

        // 2) SameProject：产出之间同项目连边
        for i in 0..produced.len() {
            let Some(pid) = produced[i].project_id else {
                continue;
            };
            for p in produced.iter().skip(i + 1) {
                if p.project_id != Some(pid) {
                    continue;
                }
                let key = unordered_key(produced[i].id, p.id, EdgeType::SameProject);
                if !seen.insert(key) {
                    continue;
                }
                if self
                    .edge_exists(edge_store, produced[i].id, p.id, EdgeType::SameProject)
                    .await?
                {
                    continue;
                }
                let edge = MemoryEdge::new(
                    produced[i].id,
                    p.id,
                    EdgeType::SameProject,
                    0.5,
                    "dream:same-project".to_string(),
                )
                .auto_generated();
                edge_store.save(&edge).await?;
                stats.record(EdgeType::SameProject);
            }
        }

        // 3) References：产出 → 来源源记忆
        for (src, tgt) in references {
            if src == tgt {
                continue;
            }
            let key = (*src, *tgt, EdgeType::References);
            if !seen.insert(key) {
                continue;
            }
            if self
                .edge_exists(edge_store, *src, *tgt, EdgeType::References)
                .await?
            {
                continue;
            }
            let edge = MemoryEdge::new(
                *src,
                *tgt,
                EdgeType::References,
                0.7,
                "dream:provenance".to_string(),
            )
            .auto_generated();
            edge_store.save(&edge).await?;
            stats.record(EdgeType::References);
        }

        // 4) Contradicts：簇内矛盾对
        for (a, b, reason) in contradictions {
            if a == b {
                continue;
            }
            let key = unordered_key(*a, *b, EdgeType::Contradicts);
            if !seen.insert(key) {
                continue;
            }
            if self
                .edge_exists(edge_store, *a, *b, EdgeType::Contradicts)
                .await?
            {
                continue;
            }
            let edge = MemoryEdge::new(
                *a,
                *b,
                EdgeType::Contradicts,
                0.9,
                format!("dream:contradiction:{}", reason),
            )
            .auto_generated();
            edge_store.save(&edge).await?;
            stats.record(EdgeType::Contradicts);
        }

        Ok(stats)
    }

    /// 检查无向/有向边是否已存在（忽略方向，容忍历史写入顺序）。
    async fn edge_exists(
        &self,
        edge_store: &Arc<dyn EdgeStorage>,
        a: Uuid,
        b: Uuid,
        edge_type: EdgeType,
    ) -> Result<bool> {
        let a_type = self.config_kind(edge_type);
        let out_a = edge_store.list_out_edges(&a).await?;
        if out_a.iter().any(|e| e.target_id == b && e.edge_type == edge_type) {
            return Ok(true);
        }
        if !a_type {
            // 无向边：反向也视为已存在
            let out_b = edge_store.list_out_edges(&b).await?;
            if out_b.iter().any(|e| e.target_id == a && e.edge_type == edge_type) {
                return Ok(true);
            }
            let in_a = edge_store.list_in_edges(&a).await?;
            if in_a.iter().any(|e| e.source_id == b && e.edge_type == edge_type) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 是否无向边。
    fn config_kind(&self, edge_type: EdgeType) -> bool {
        edge_type.is_directed()
    }
}

/// 无向边去重键（小 id 在前）。
fn unordered_key(a: Uuid, b: Uuid, edge_type: EdgeType) -> (Uuid, Uuid, EdgeType) {
    if a <= b {
        (a, b, edge_type)
    } else {
        (b, a, edge_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{EdgeStore, RocksDBStore};
    use mr_common::MemoryType;
    use std::sync::Arc;

    fn make_edge_store() -> (Arc<dyn EdgeStorage>, tempfile::TempDir, Arc<RocksDBStore>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(RocksDBStore::open(dir.path()).unwrap());
        (Arc::new(EdgeStore::new(db.clone())), dir, db)
    }

    fn mem(content: &str, project: Option<Uuid>) -> Memory {
        let mut m = Memory::new(content.to_string(), MemoryType::Knowledge);
        m.project_id = project;
        m
    }

    #[tokio::test]
    async fn test_build_edges_minimal() {
        let (store, _dir, _db) = make_edge_store();
        let builder = EdgeBuilder::new(DreamConfig::default());

        let p = mem("fact one", None);
        let s1 = mem("source one", None);
        let s2 = mem("source two", None);

        let mut emb = HashMap::new();
        emb.insert(p.id, vec![1.0, 0.0]);
        emb.insert(s1.id, vec![0.9, 0.1]);
        emb.insert(s2.id, vec![0.0, 1.0]);

        let produced = vec![p.clone()];
        let pool = vec![p.clone(), s1.clone(), s2.clone()];

        let refs = vec![(p.id, s1.id)];
        let contra = vec![(p.id, s2.id, "conflict".to_string())];

        let stats = builder
            .build(&produced, &emb, &pool, &emb, &refs, &contra, &store)
            .await
            .unwrap();
        assert!(stats.total >= 3);
        assert!(stats.by_type.contains_key("references"));
        assert!(stats.by_type.contains_key("contradicts"));
    }

    #[tokio::test]
    async fn test_same_project_edge() {
        let (store, _dir, _db) = make_edge_store();
        let builder = EdgeBuilder::new(DreamConfig::default());
        let pid = Uuid::new_v4();
        let a = mem("a", Some(pid));
        let b = mem("b", Some(pid));
        let mut emb = HashMap::new();
        emb.insert(a.id, vec![1.0, 0.0]);
        emb.insert(b.id, vec![0.0, 1.0]);
        let stats = builder
            .build(
                &[a.clone(), b.clone()],
                &emb,
                &[a.clone(), b.clone()],
                &emb,
                &[],
                &[],
                &store,
            )
            .await
            .unwrap();
        assert!(stats.by_type.contains_key("same_project"));
    }

    #[tokio::test]
    async fn test_no_duplicate_edges_on_rerun() {
        let (store, _dir, _db) = make_edge_store();
        let builder = EdgeBuilder::new(DreamConfig::default());
        let p = mem("fact", None);
        let s = mem("src", None);
        let mut emb = HashMap::new();
        emb.insert(p.id, vec![1.0, 0.0]);
        emb.insert(s.id, vec![1.0, 0.0]);
        let refs = vec![(p.id, s.id)];

        let first = builder
            .build(
                &[p.clone()],
                &emb,
                &[p.clone(), s.clone()],
                &emb,
                &refs,
                &[],
                &store,
            )
            .await
            .unwrap();
        let second = builder
            .build(
                &[p.clone()],
                &emb,
                &[p.clone(), s.clone()],
                &emb,
                &refs,
                &[],
                &store,
            )
            .await
            .unwrap();
        assert!(first.total >= 1);
        assert_eq!(second.total, 0, "重跑不应产生新边");
    }
}
