//! # 边存储实现
//!
//! [`EdgeStore`] 基于 [`RocksDBStore`] 实现 [`EdgeStorage`] trait，
//! 提供记忆图边的完整生命周期管理和图遍历扩散功能。
//!
//! ## 存储结构
//!
//! - **edges 列族**：UUID → JSON 序列化的 `MemoryEdge`
//! - **node_edges_out 列族**：`source_id:edge_type` → `[edge_ids]`（出边索引）
//! - **node_edges_in 列族**：`target_id:edge_type` → `[edge_ids]`（入边索引）

use super::rocksdb::RocksDBStore;
use super::traits::{EdgeStorage, GraphSearchHit, GraphTraversalParams};
use anyhow::{Context, Result};
use async_trait::async_trait;
use mr_common::{EdgeType, MemoryEdge};
use std::collections::HashSet;
use uuid::Uuid;

pub struct EdgeStore {
    rocksdb: RocksDBStore,
}

impl EdgeStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }

    fn edge_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }

    fn out_edge_key(source_id: &Uuid, edge_type: EdgeType) -> Vec<u8> {
        format!("{}:{}", source_id, edge_type).into_bytes()
    }

    fn in_edge_key(target_id: &Uuid, edge_type: EdgeType) -> Vec<u8> {
        format!("{}:{}", target_id, edge_type).into_bytes()
    }

    fn serialize_edge(edge: &MemoryEdge) -> Result<Vec<u8>> {
        serde_json::to_vec(edge).context("Failed to serialize edge")
    }

    fn deserialize_edge(data: &[u8]) -> Result<MemoryEdge> {
        serde_json::from_slice(data).context("Failed to deserialize edge")
    }

    fn serialize_edge_ids(ids: &[Uuid]) -> Result<Vec<u8>> {
        serde_json::to_vec(ids).context("Failed to serialize edge ids")
    }

    fn deserialize_edge_ids(data: &[u8]) -> Result<Vec<Uuid>> {
        serde_json::from_slice(data).context("Failed to deserialize edge ids")
    }

    async fn get_edge_ids_from_cf(&self, key: &[u8], cf_name: &str) -> Result<Vec<Uuid>> {
        let bytes = {
            let cf = match cf_name {
                "out" => self.rocksdb.cf_node_edges_out()?,
                "in" => self.rocksdb.cf_node_edges_in()?,
                _ => return Ok(Vec::new()),
            };
            self.rocksdb.get_cf(cf, key)?
        };

        match bytes {
            Some(data) => Self::deserialize_edge_ids(&data),
            None => Ok(Vec::new()),
        }
    }

    async fn add_edge_to_index(&self, key: &[u8], edge_id: Uuid, cf_name: &str) -> Result<()> {
        let ids = self.get_edge_ids_from_cf(key, cf_name).await?;

        let mut new_ids = ids;
        if !new_ids.contains(&edge_id) {
            new_ids.push(edge_id);
            let data = Self::serialize_edge_ids(&new_ids)?;

            let cf = match cf_name {
                "out" => self.rocksdb.cf_node_edges_out()?,
                "in" => self.rocksdb.cf_node_edges_in()?,
                _ => return Ok(()),
            };
            self.rocksdb.put_cf(cf, key, &data)?;
        }

        Ok(())
    }

    async fn remove_edge_from_index(
        &self,
        key: &[u8],
        edge_id: &Uuid,
        cf_name: &str,
    ) -> Result<()> {
        let ids = self.get_edge_ids_from_cf(key, cf_name).await?;

        let mut new_ids = ids;
        new_ids.retain(|id| id != edge_id);

        let cf = match cf_name {
            "out" => self.rocksdb.cf_node_edges_out()?,
            "in" => self.rocksdb.cf_node_edges_in()?,
            _ => return Ok(()),
        };

        if new_ids.is_empty() {
            self.rocksdb.delete_cf(cf, key)?;
        } else {
            let data = Self::serialize_edge_ids(&new_ids)?;
            self.rocksdb.put_cf(cf, key, &data)?;
        }

        Ok(())
    }
}

#[async_trait]
impl EdgeStorage for EdgeStore {
    async fn save(&self, edge: &MemoryEdge) -> Result<()> {
        let id_key = Self::edge_key(&edge.id);
        let data = Self::serialize_edge(edge)?;

        let cf_edges = self.rocksdb.cf_edges()?;
        self.rocksdb.put_cf(cf_edges, &id_key, &data)?;

        let out_key = Self::out_edge_key(&edge.source_id, edge.edge_type);
        self.add_edge_to_index(&out_key, edge.id, "out").await?;

        let in_key = Self::in_edge_key(&edge.target_id, edge.edge_type);
        self.add_edge_to_index(&in_key, edge.id, "in").await?;

        Ok(())
    }

    async fn get(&self, id: &Uuid) -> Result<Option<MemoryEdge>> {
        let id_key = Self::edge_key(id);
        let cf_edges = self.rocksdb.cf_edges()?;

        match self.rocksdb.get_cf(cf_edges, &id_key)? {
            Some(bytes) => {
                let edge = Self::deserialize_edge(&bytes)?;
                Ok(Some(edge))
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let edge = self.get(id).await?;

        match edge {
            Some(e) => {
                let id_key = Self::edge_key(id);
                let cf_edges = self.rocksdb.cf_edges()?;
                self.rocksdb.delete_cf(cf_edges, &id_key)?;

                let out_key = Self::out_edge_key(&e.source_id, e.edge_type);
                self.remove_edge_from_index(&out_key, id, "out").await?;

                let in_key = Self::in_edge_key(&e.target_id, e.edge_type);
                self.remove_edge_from_index(&in_key, id, "in").await?;

                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn list_out_edges(&self, source_id: &Uuid) -> Result<Vec<MemoryEdge>> {
        let mut edges = Vec::new();

        for edge_type in [
            EdgeType::Similar,
            EdgeType::CausedBy,
            EdgeType::References,
            EdgeType::SameProject,
            EdgeType::SameSession,
            EdgeType::Contradicts,
            EdgeType::Evolves,
        ] {
            let key = Self::out_edge_key(source_id, edge_type);
            let ids = self.get_edge_ids_from_cf(&key, "out").await?;

            for id in ids {
                if let Some(edge) = self.get(&id).await? {
                    edges.push(edge);
                }
            }
        }

        Ok(edges)
    }

    async fn list_in_edges(&self, target_id: &Uuid) -> Result<Vec<MemoryEdge>> {
        let mut edges = Vec::new();

        for edge_type in [
            EdgeType::Similar,
            EdgeType::CausedBy,
            EdgeType::References,
            EdgeType::SameProject,
            EdgeType::SameSession,
            EdgeType::Contradicts,
            EdgeType::Evolves,
        ] {
            let key = Self::in_edge_key(target_id, edge_type);
            let ids = self.get_edge_ids_from_cf(&key, "in").await?;

            for id in ids {
                if let Some(edge) = self.get(&id).await? {
                    edges.push(edge);
                }
            }
        }

        Ok(edges)
    }

    async fn list_by_type(&self, edge_type: EdgeType) -> Result<Vec<MemoryEdge>> {
        let cf_edges = self.rocksdb.cf_edges()?;
        let mut iter = self.rocksdb.iter_cf(cf_edges);

        let mut edges = Vec::new();
        iter.seek_to_first();
        while iter.valid() {
            if let Some(value) = iter.value() {
                if let Ok(edge) = Self::deserialize_edge(value) {
                    if edge.edge_type == edge_type {
                        edges.push(edge);
                    }
                }
            }
            iter.next();
        }

        Ok(edges)
    }

    async fn count(&self) -> Result<usize> {
        let cf_edges = self.rocksdb.cf_edges()?;
        let mut iter = self.rocksdb.iter_cf(cf_edges);

        let mut count = 0;
        iter.seek_to_first();
        while iter.valid() {
            if iter.value().is_some() {
                count += 1;
            }
            iter.next();
        }

        Ok(count)
    }

    async fn neighbors(&self, memory_id: &Uuid) -> Result<Vec<MemoryEdge>> {
        let mut all_edges = self.list_out_edges(memory_id).await?;
        let in_edges = self.list_in_edges(memory_id).await?;

        for edge in in_edges {
            if !all_edges.iter().any(|e| e.id == edge.id) {
                all_edges.push(edge);
            }
        }

        Ok(all_edges)
    }

    async fn traverse(
        &self,
        seed_ids: &[Uuid],
        params: GraphTraversalParams,
    ) -> Result<Vec<GraphSearchHit>> {
        let mut visited: HashSet<Uuid> = HashSet::new();
        let mut results: Vec<GraphSearchHit> = Vec::new();
        let mut current_frontier: Vec<(Uuid, f32, Vec<Uuid>, Vec<EdgeType>)> = seed_ids
            .iter()
            .map(|id| (*id, 1.0, vec![*id], vec![]))
            .collect();

        for depth in 0..params.max_depth {
            if current_frontier.is_empty() {
                break;
            }

            let mut next_frontier: Vec<(Uuid, f32, Vec<Uuid>, Vec<EdgeType>)> = Vec::new();

            for (memory_id, base_score, path, path_types) in current_frontier {
                if visited.contains(&memory_id) {
                    continue;
                }
                visited.insert(memory_id);

                if base_score >= params.min_score && path.len() > 1 {
                    results.push(GraphSearchHit {
                        memory_id,
                        score: base_score,
                        path: path.clone(),
                        path_types: path_types.clone(),
                    });
                }

                if depth < params.max_depth - 1 {
                    let neighbor_edges = self.neighbors(&memory_id).await?;

                    for edge in neighbor_edges {
                        let (neighbor_id, _direction) = if edge.source_id == memory_id {
                            (edge.target_id, "out")
                        } else {
                            (edge.source_id, "in")
                        };

                        if visited.contains(&neighbor_id) {
                            continue;
                        }

                        if let Some(ref allowed_types) = params.edge_types {
                            if !allowed_types.contains(&edge.edge_type) {
                                continue;
                            }
                        }

                        let type_weight = edge.edge_type.diffusion_weight();
                        let edge_score = base_score * edge.weight * type_weight * params.decay;

                        if edge_score >= params.min_score {
                            let mut new_path = path.clone();
                            new_path.push(neighbor_id);
                            let mut new_types = path_types.clone();
                            new_types.push(edge.edge_type);

                            next_frontier.push((neighbor_id, edge_score, new_path, new_types));
                        }
                    }
                }
            }

            current_frontier = next_frontier;

            if results.len() >= params.max_results {
                break;
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.truncate(params.max_results);

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_store() -> EdgeStore {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        EdgeStore::new(rocksdb)
    }

    #[tokio::test]
    async fn test_edge_save_and_get() {
        let store = create_test_store().await;

        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let edge = MemoryEdge::new(source, target, EdgeType::Similar, 0.85, "test".to_string());

        store.save(&edge).await.unwrap();

        let retrieved = store.get(&edge.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().weight, 0.85);
    }

    #[tokio::test]
    async fn test_edge_list_out_edges() {
        let store = create_test_store().await;

        let source = Uuid::new_v4();
        let target1 = Uuid::new_v4();
        let target2 = Uuid::new_v4();

        let edge1 = MemoryEdge::new(source, target1, EdgeType::Similar, 0.9, "".to_string());
        let edge2 = MemoryEdge::new(source, target2, EdgeType::SameProject, 1.0, "".to_string());

        store.save(&edge1).await.unwrap();
        store.save(&edge2).await.unwrap();

        let edges = store.list_out_edges(&source).await.unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[tokio::test]
    async fn test_edge_neighbors() {
        let store = create_test_store().await;

        let node_a = Uuid::new_v4();
        let node_b = Uuid::new_v4();
        let node_c = Uuid::new_v4();

        let edge_ab = MemoryEdge::new(node_a, node_b, EdgeType::Similar, 0.8, "".to_string());
        let edge_bc = MemoryEdge::new(node_b, node_c, EdgeType::CausedBy, 0.7, "".to_string());

        store.save(&edge_ab).await.unwrap();
        store.save(&edge_bc).await.unwrap();

        let neighbors_a = store.neighbors(&node_a).await.unwrap();
        assert_eq!(neighbors_a.len(), 1);

        let neighbors_b = store.neighbors(&node_b).await.unwrap();
        assert_eq!(neighbors_b.len(), 2);
    }

    #[tokio::test]
    async fn test_edge_traverse() {
        let store = create_test_store().await;

        let node_a = Uuid::new_v4();
        let node_b = Uuid::new_v4();
        let node_c = Uuid::new_v4();

        let edge_ab = MemoryEdge::new(node_a, node_b, EdgeType::Similar, 0.9, "".to_string());
        let edge_bc = MemoryEdge::new(node_b, node_c, EdgeType::Similar, 0.8, "".to_string());

        store.save(&edge_ab).await.unwrap();
        store.save(&edge_bc).await.unwrap();

        let params = GraphTraversalParams {
            max_depth: 3,
            decay: 0.6,
            min_score: 0.1,
            max_results: 10,
            edge_types: None,
        };

        let results = store.traverse(&[node_a], params).await.unwrap();
        assert!(!results.is_empty());

        let node_b_hit = results.iter().find(|h| h.memory_id == node_b);
        assert!(node_b_hit.is_some());

        let node_c_hit = results.iter().find(|h| h.memory_id == node_c);
        assert!(node_c_hit.is_some());
    }

    #[tokio::test]
    async fn test_edge_delete() {
        let store = create_test_store().await;

        let edge = MemoryEdge::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            EdgeType::Similar,
            0.8,
            "".to_string(),
        );
        store.save(&edge).await.unwrap();

        let deleted = store.delete(&edge.id).await.unwrap();
        assert!(deleted);

        let retrieved = store.get(&edge.id).await.unwrap();
        assert!(retrieved.is_none());
    }
}
