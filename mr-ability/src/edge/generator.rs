//! # 边生成器
//!
//! [`EdgeGenerator`] trait 定义边自动生成的接口，
//! [`MockEdgeGenerator`] 提供基于规则的测试实现。
//!
//! ## 边生成策略
//!
//! - **SameProject**: 基于元数据自动生成，无需 LLM
//! - **Similar/CausedBy/Contradicts**: 语义关系，需要 LLM 判断
//! - **Evolves/References**: 演进/引用关系，需要 LLM 判断

use crate::storage::{EdgeStorage, MemoryStorage};
use anyhow::Result;
use async_trait::async_trait;
use mr_common::{EdgeType, Memory, MemoryEdge};
use uuid::Uuid;

/// 边生成器 trait。
///
/// 根据新记忆和候选记忆列表，自动生成边关系。
#[async_trait]
pub trait EdgeGenerator: Send + Sync {
    /// 为新记忆生成边关系。
    ///
    /// # 参数
    ///
    /// - `new_memory`: 新添加的记忆
    /// - `candidate_memories`: 候选关联记忆（通常通过向量检索获得）
    /// - `edge_store`: 边存储
    ///
    /// # 返回
    ///
    /// 生成的边列表。
    async fn generate(
        &self,
        new_memory: &Memory,
        candidate_memories: &[Memory],
        edge_store: &dyn EdgeStorage,
    ) -> Result<Vec<MemoryEdge>>;
}

/// Mock 边生成器。
///
/// 基于关键词匹配规则生成边，用于测试。
pub struct MockEdgeGenerator {
    min_similarity: f32,
    max_edges_per_memory: usize,
}

impl MockEdgeGenerator {
    pub fn new(min_similarity: f32, max_edges_per_memory: usize) -> Self {
        Self {
            min_similarity: min_similarity.clamp(0.0, 1.0),
            max_edges_per_memory,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(0.5, 10)
    }

    fn calculate_similarity(content_a: &str, content_b: &str) -> f32 {
        let a_lower = content_a.to_lowercase();
        let b_lower = content_b.to_lowercase();

        let words_a: std::collections::HashSet<&str> = a_lower.split_whitespace().collect();
        let words_b: std::collections::HashSet<&str> = b_lower.split_whitespace().collect();

        if words_a.is_empty() || words_b.is_empty() {
            return 0.0;
        }

        let intersection = words_a.intersection(&words_b).count();
        let union = words_a.union(&words_b).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    fn infer_edge_type(&self, _content_a: &str, _content_b: &str) -> EdgeType {
        EdgeType::Similar
    }

    fn generate_evidence(edge_type: EdgeType, similarity: f32) -> String {
        format!(
            "Auto-generated {} edge (similarity: {:.2})",
            edge_type, similarity
        )
    }
}

#[async_trait]
impl EdgeGenerator for MockEdgeGenerator {
    async fn generate(
        &self,
        new_memory: &Memory,
        candidate_memories: &[Memory],
        edge_store: &dyn EdgeStorage,
    ) -> Result<Vec<MemoryEdge>> {
        let existing_edges = edge_store.neighbors(&new_memory.id).await?;
        let existing_targets: std::collections::HashSet<Uuid> = existing_edges
            .iter()
            .map(|e| {
                if e.source_id == new_memory.id {
                    e.target_id
                } else {
                    e.source_id
                }
            })
            .collect();

        let mut edges = Vec::new();
        let mut edge_count = existing_edges.len();

        for candidate in candidate_memories {
            if edge_count >= self.max_edges_per_memory {
                break;
            }

            if existing_targets.contains(&candidate.id) {
                continue;
            }

            if new_memory.id == candidate.id {
                continue;
            }

            let similarity = Self::calculate_similarity(&new_memory.content, &candidate.content);

            if similarity >= self.min_similarity {
                let edge_type = self.infer_edge_type(&new_memory.content, &candidate.content);
                let evidence = Self::generate_evidence(edge_type, similarity);

                let edge =
                    MemoryEdge::new(new_memory.id, candidate.id, edge_type, similarity, evidence)
                        .auto_generated();

                edges.push(edge);
                edge_count += 1;
            }
        }

        Ok(edges)
    }
}

/// LLM 语义边生成器（P2-8，**默认关**）。
///
/// 由 LLM 判断新记忆与候选记忆之间的**有向语义关系**
/// （`CausedBy` / `Evolves` / `References`），并给出置信度。
///
/// ## 默认关闭
///
/// 语义边成本高（每次调用 LLM）且需灰度验证，故由
/// `DreamConfig.networking_semantic_enabled`（默认 `false`）控制是否启用；
/// 仅在显式开启且注入 LLM 时才参与 Stage N。
///
/// ## 容错
///
/// LLM 调用失败或 JSON 解析失败时返回 `Ok(vec![])`（不产生边、不中断
/// Dream），保证语义边是**增强项**而非单点故障源。
pub struct LlmEdgeGenerator {
    llm: std::sync::Arc<dyn crate::llm::LlmClient>,
    max_edges: usize,
    min_confidence: f32,
}

/// LLM 返回的单条关系。
#[derive(Debug, Clone, serde::Deserialize)]
struct LlmRelation {
    /// 候选记忆在列表中的下标
    target: usize,
    /// 关系类型：caused_by / evolves / references
    #[serde(rename = "type")]
    relation_type: String,
    /// 置信度 0.0~1.0
    #[serde(default = "default_confidence")]
    confidence: f32,
    /// 关系依据（简述）
    #[serde(default)]
    reason: String,
}

fn default_confidence() -> f32 {
    0.5
}

impl LlmEdgeGenerator {
    /// 创建语义边生成器。
    ///
    /// `max_edges`：单条新记忆最多产生的语义边数；`min_confidence`：采纳阈值。
    pub fn new(
        llm: std::sync::Arc<dyn crate::llm::LlmClient>,
        max_edges: usize,
        min_confidence: f32,
    ) -> Self {
        Self {
            llm,
            max_edges,
            min_confidence,
        }
    }

    /// 解析关系类型字符串。
    fn parse_type(s: &str) -> Option<EdgeType> {
        match s.to_lowercase().as_str() {
            "caused_by" => Some(EdgeType::CausedBy),
            "evolves" => Some(EdgeType::Evolves),
            "references" => Some(EdgeType::References),
            _ => None,
        }
    }

    /// 从 LLM 输出中提取 JSON 数组并解析（容忍 ```json 代码块包裹）。
    fn parse_relations(raw: &str) -> Vec<LlmRelation> {
        let cleaned = raw.trim();
        let cleaned = cleaned
            .strip_prefix("```json")
            .or_else(|| cleaned.strip_prefix("```"))
            .unwrap_or(cleaned)
            .trim_end()
            .strip_suffix("```")
            .unwrap_or(cleaned)
            .trim();
        let (start, end) = match (cleaned.find('['), cleaned.rfind(']')) {
            (Some(s), Some(e)) if e >= s => (s, e),
            _ => return Vec::new(),
        };
        // LLM 输出格式异常时不再静默返回空数组：记录原始片段便于排查
        // （此前 `unwrap_or_default()` 会让语义边生成静默失效）。
        match serde_json::from_str(&cleaned[start..=end]) {
            Ok(relations) => relations,
            Err(e) => {
                // 按字符截断，避免在 UTF-8 多字节边界上切片导致 panic。
                let snippet: String = cleaned[start..].chars().take(200).collect();
                tracing::warn!(
                    target: "edge",
                    "parse_relations: LLM 输出 JSON 解析失败，返回空边集: {}; raw={:?}",
                    e,
                    snippet
                );
                Vec::new()
            }
        }
    }
}

#[async_trait]
impl EdgeGenerator for LlmEdgeGenerator {
    async fn generate(
        &self,
        new_memory: &Memory,
        candidate_memories: &[Memory],
        edge_store: &dyn EdgeStorage,
    ) -> Result<Vec<MemoryEdge>> {
        if candidate_memories.is_empty() || self.max_edges == 0 {
            return Ok(Vec::new());
        }

        // 已存在邻居关系，避免重复建边
        let existing = edge_store.neighbors(&new_memory.id).await?;
        let existing_targets: std::collections::HashSet<Uuid> = existing
            .iter()
            .map(|e| {
                if e.source_id == new_memory.id {
                    e.target_id
                } else {
                    e.source_id
                }
            })
            .collect();

        let mut listing = String::new();
        for (i, c) in candidate_memories.iter().enumerate() {
            listing.push_str(&format!(
                "{}. [{}]\n",
                i,
                c.content.chars().take(200).collect::<String>()
            ));
        }

        let system = "你是记忆关系分析引擎。判断给定记忆与候选记忆之间的有向语义关系。\
只输出 JSON 数组，元素形如 {\"target\":<候选下标>,\"type\":\"caused_by|evolves|references\",\"confidence\":0.0~1.0,\"reason\":\"简述\"}。\
只保留确有关系的候选；无关系则不输出。禁止输出数组以外的任何内容。";
        let user = format!(
            "新记忆：\n{}\n\n候选记忆：\n{}",
            new_memory.content.chars().take(500).collect::<String>(),
            listing
        );
        let messages = vec![
            crate::llm::LlmMessage::system(system),
            crate::llm::LlmMessage::user(user),
        ];

        let raw = match self
            .llm
            .chat_with_think(&messages, mr_common::ThinkLevel::Off)
            .await
        {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let relations = Self::parse_relations(&raw);
        let mut edges = Vec::new();
        for rel in relations {
            if edges.len() >= self.max_edges {
                break;
            }
            if rel.confidence < self.min_confidence {
                continue;
            }
            let candidate = match candidate_memories.get(rel.target) {
                Some(c) => c,
                None => continue,
            };
            // 跳过自环：候选池包含新记忆自身，LLM 常会返回“内容一致”的自指关系。
            if candidate.id == new_memory.id {
                continue;
            }
            let Some(edge_type) = Self::parse_type(&rel.relation_type) else {
                continue;
            };
            if existing_targets.contains(&candidate.id) {
                continue;
            }
            let edge = MemoryEdge::new(
                new_memory.id,
                candidate.id,
                edge_type,
                rel.confidence.clamp(0.0, 1.0),
                format!("dream:llm-semantic:{}", rel.reason),
            )
            .auto_generated();
            edges.push(edge);
        }

        Ok(edges)
    }
}

/// 项目边生成器。
///
/// 基于 `project_id` 元数据自动生成 SameProject 边。
pub struct ProjectEdgeGenerator {
    max_edges_per_project: usize,
}

impl ProjectEdgeGenerator {
    pub fn new(max_edges_per_project: usize) -> Self {
        Self {
            max_edges_per_project,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(5)
    }

    pub async fn generate_project_edges(
        &self,
        memory: &Memory,
        memory_store: &dyn MemoryStorage,
        edge_store: &dyn EdgeStorage,
    ) -> Result<Vec<MemoryEdge>> {
        let project_id = match memory.project_id {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };

        let project_memories = memory_store.list_by_project(&project_id).await?;

        let existing = edge_store.neighbors(&memory.id).await?;
        let existing_ids: std::collections::HashSet<Uuid> = existing
            .iter()
            .filter(|e| e.edge_type == EdgeType::SameProject)
            .map(|e| {
                if e.source_id == memory.id {
                    e.target_id
                } else {
                    e.source_id
                }
            })
            .collect();

        let mut edges = Vec::new();
        for other in project_memories {
            if other.id == memory.id {
                continue;
            }
            if existing_ids.contains(&other.id) {
                continue;
            }
            if edges.len() >= self.max_edges_per_project {
                break;
            }
            edges.push(
                MemoryEdge::new(
                    memory.id,
                    other.id,
                    EdgeType::SameProject,
                    1.0,
                    format!("Same project: {:?}", memory.project_id),
                )
                .auto_generated(),
            );
        }

        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{EdgeStore, RocksDBStore};
    use mr_common::MemoryType;
    use tempfile::tempdir;

    async fn create_test_edge_store() -> EdgeStore {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        EdgeStore::new(std::sync::Arc::new(rocksdb))
    }

    fn create_test_memory(content: &str) -> Memory {
        Memory::new(content.to_string(), MemoryType::Knowledge)
    }

    #[tokio::test]
    async fn test_llm_edge_generator_parses_relations() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        let llm = std::sync::Arc::new(MockLlmClient::new(
            r#"[{"target":0,"type":"caused_by","confidence":0.9,"reason":"a caused b"}]"#,
        ));
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);

        let a = create_test_memory("root cause of the crash");
        let b = create_test_memory("crash observed in production");
        let edges = generator
            .generate(&a, &[b.clone()], &edge_store)
            .await
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, EdgeType::CausedBy);
    }

    #[tokio::test]
    async fn test_llm_edge_generator_skips_self_loop() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        // 候选列表同时包含新记忆自身与另一条记忆
        let llm = std::sync::Arc::new(MockLlmClient::new(
            r#"[{"target":0,"type":"references","confidence":0.99,"reason":"same memory"},{"target":1,"type":"evolves","confidence":0.9,"reason":"b evolves a"}]"#,
        ));
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);

        let a = create_test_memory("memory a");
        let b = create_test_memory("memory b");
        let edges = generator
            .generate(&a, &[a.clone(), b.clone()], &edge_store)
            .await
            .unwrap();
        assert_eq!(edges.len(), 1, "自环应被跳过，仅保留指向 b 的边");
        assert_eq!(edges[0].target_id, b.id);
    }

    #[tokio::test]
    async fn test_llm_edge_generator_filters_low_confidence_and_bad_type() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        let llm = std::sync::Arc::new(MockLlmClient::new(
            r#"```json
[{"target":0,"type":"caused_by","confidence":0.2},{"target":1,"type":"unknown","confidence":0.9}]
```"#,
        ));
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);
        let a = create_test_memory("a");
        let edges = generator
            .generate(
                &a,
                &[create_test_memory("b"), create_test_memory("c")],
                &edge_store,
            )
            .await
            .unwrap();
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn test_llm_edge_generator_llm_failure_is_non_fatal() {
        use crate::llm::MockLlmClient;
        let edge_store = create_test_edge_store().await;
        let llm = std::sync::Arc::new(MockLlmClient::unconfigured());
        let generator = LlmEdgeGenerator::new(llm, 3, 0.6);
        let a = create_test_memory("a");
        let edges = generator
            .generate(&a, &[create_test_memory("b")], &edge_store)
            .await
            .unwrap();
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn test_mock_generator_similar_edges() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::with_defaults();

        let memory_a = create_test_memory("Tokio is a great async runtime for Rust");
        let memory_b = create_test_memory("async-std is another async runtime for Rust");
        let memory_c = create_test_memory("Database optimization techniques");

        let edges = generator
            .generate(&memory_a, &[memory_b.clone(), memory_c], &edge_store)
            .await
            .unwrap();

        assert!(!edges.is_empty());
        assert!(edges[0].edge_type == EdgeType::Similar);
        assert!(edges[0].auto_generated);
    }

    #[tokio::test]
    async fn test_mock_generator_respects_max_edges() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::new(0.3, 2);

        let memory = create_test_memory("Test memory with keywords");
        let candidates: Vec<Memory> = (0..10)
            .map(|i| create_test_memory(&format!("Test memory with keywords {}", i)))
            .collect();

        let edges = generator
            .generate(&memory, &candidates, &edge_store)
            .await
            .unwrap();
        assert!(edges.len() <= 2);
    }

    #[tokio::test]
    async fn test_mock_generator_avoids_self_edge() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::with_defaults();

        let memory = create_test_memory("Test content");
        let edges = generator
            .generate(&memory, std::slice::from_ref(&memory), &edge_store)
            .await
            .unwrap();

        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn test_mock_generator_low_similarity_filtered() {
        let edge_store = create_test_edge_store().await;
        let generator = MockEdgeGenerator::new(0.8, 10);

        let memory_a = create_test_memory("Rust programming language");
        let memory_b = create_test_memory("Cooking recipes for dinner");

        let edges = generator
            .generate(&memory_a, &[memory_b], &edge_store)
            .await
            .unwrap();

        assert!(edges.is_empty());
    }
}
