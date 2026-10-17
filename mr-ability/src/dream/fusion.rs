//! # Stage F —— 融合（Fusion）
//!
//! 实现 Dream 两段式管线的第一段：把候选记忆按语义聚成若干聚焦主题的簇，
//! 再对每簇做一次 LLM 结构化提炼（事实 / 摘要 / 矛盾）。
//!
//! ## 设计要点
//!
//! - **零 LLM 聚类**：复用已有向量，按重要性顺序 + 相似度阈值连通成簇，结果确定性可复现；
//! - **分簇提炼**：每簇一次 LLM 调用，要求 JSON 结构化输出；解析失败回退 Markdown 摘要；
//! - **无向量退化**：向量不可用时按 `project_id + memory_type` 分组。
//!
//! 命名正交：本模块不复用 `Scene`。

use std::collections::HashMap;
use std::sync::Arc;

use mr_common::Memory;
use serde::Deserialize;
use tracing::warn;
use uuid::Uuid;

use crate::llm::{LlmClient, LlmMessage};

/// 簇内提炼出的一条事实。
#[derive(Debug, Clone, Deserialize)]
pub struct FactItem {
    /// 事实正文（独立、可检索、保留原始表述）
    pub text: String,
    /// 来源线索（文件路径/行号/数字等）
    #[serde(default)]
    pub source_hints: Vec<String>,
}

/// 簇内提炼出的一对矛盾。
#[derive(Debug, Clone, Deserialize)]
pub struct ContradictionItem {
    /// 簇内下标 a
    pub a: usize,
    /// 簇内下标 b
    pub b: usize,
    /// 矛盾理由
    #[serde(default)]
    pub reason: String,
}

/// 单簇结构化提炼结果。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClusterExtraction {
    /// 原子事实条
    #[serde(default)]
    pub facts: Vec<FactItem>,
    /// 跨记忆关联/规律/行动的摘要（Markdown，≤500 字）
    #[serde(default)]
    pub summary: String,
    /// 矛盾对（下标）
    #[serde(default)]
    pub contradictions: Vec<ContradictionItem>,
}

/// 聚类结果：每个簇是候选下标的有序集合。
pub type Cluster = Vec<usize>;

/// 余弦相似度（任一为零向量返回 0）。
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / (na.sqrt() * nb.sqrt())).clamp(-1.0, 1.0)
}

/// 对候选做语义聚类。
///
/// # 参数
///
/// - `candidates`：候选记忆（顺序保留，下标用于簇内引用）
/// - `embeddings`：候选 id → 向量（缺失者不参与相似度连通）
/// - `threshold`：相似度阈值
/// - `max_size`：单簇上限（超出按相似度拆分为子簇）
/// - `max_clusters`：单次最大簇数（超出合并最小的若干簇）
///
/// # 行为
///
/// 按 importance 降序取种子，未归属者若与种子相似度 ≥ 阈值则并入；
/// 全部候选先按"项目 + 类型"粗分（无向量时的退化分组），再在组内做相似度连通。
/// 结果确定性：种子顺序固定，遍历顺序固定。
pub fn cluster_candidates(
    candidates: &[Memory],
    embeddings: &HashMap<Uuid, Vec<f32>>,
    threshold: f32,
    max_size: usize,
    max_clusters: usize,
) -> Vec<Cluster> {
    // 无向量可用：退化为按 (project_id, memory_type) 分组
    if embeddings.is_empty() {
        return group_by_project_type(candidates);
    }

    // 种子顺序：importance 降序（稳定：相等时按原下标）
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|&a, &b| {
        candidates[b]
            .importance
            .partial_cmp(&candidates[a].importance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });

    let mut assigned = vec![false; candidates.len()];
    let mut clusters: Vec<Cluster> = Vec::new();

    for &seed in &order {
        if assigned[seed] {
            continue;
        }
        assigned[seed] = true;
        let mut cluster = vec![seed];

        if let Some(seed_vec) = embeddings.get(&candidates[seed].id) {
            for &other in &order {
                if assigned[other] {
                    continue;
                }
                if let Some(other_vec) = embeddings.get(&candidates[other].id) {
                    if cosine(seed_vec, other_vec) >= threshold {
                        assigned[other] = true;
                        cluster.push(other);
                    }
                }
            }
        }

        // 超上限：按与种子的相似度拆分
        if max_size > 0 && cluster.len() > max_size {
            for part in split_cluster(&cluster, candidates, embeddings, seed, max_size) {
                clusters.push(part);
            }
        } else {
            clusters.push(cluster);
        }
    }

    // 超过最大簇数：合并相似度最高、体量最小的尾部簇
    while max_clusters > 0 && clusters.len() > max_clusters {
        // 找到体量最小的簇，并入与之最相似的另一个簇
        clusters.sort_by(|a, b| a.len().cmp(&b.len()));
        let smallest = clusters.remove(0);
        if let Some(best) = best_merge_target(&smallest, &clusters, candidates, embeddings) {
            clusters[best].extend(smallest);
        } else {
            clusters.push(smallest);
            break;
        }
    }

    clusters
}

/// 按 (project_id, memory_type) 退化分组。
fn group_by_project_type(candidates: &[Memory]) -> Vec<Cluster> {
    let mut map: HashMap<(Option<Uuid>, String), Cluster> = HashMap::new();
    let mut order: Vec<(Option<Uuid>, String)> = Vec::new();
    for (i, m) in candidates.iter().enumerate() {
        let key = (m.project_id, m.memory_type.to_string());
        let entry = map.entry(key.clone()).or_default();
        if entry.is_empty() {
            order.push(key);
        }
        entry.push(i);
    }
    order.into_iter().filter_map(|k| map.remove(&k)).collect()
}

/// 把超上限的簇按"与种子相似度"降序切成多个大小 ≤ max_size 的子簇。
fn split_cluster(
    cluster: &[usize],
    candidates: &[Memory],
    embeddings: &HashMap<Uuid, Vec<f32>>,
    seed: usize,
    max_size: usize,
) -> Vec<Cluster> {
    let mut members: Vec<usize> = cluster.to_vec();
    let seed_vec = embeddings.get(&candidates[seed].id);
    members.sort_by(|&a, &b| {
        let sa = seed_vec
            .zip(embeddings.get(&candidates[a].id))
            .map(|(s, v)| cosine(s, v))
            .unwrap_or(0.0);
        let sb = seed_vec
            .zip(embeddings.get(&candidates[b].id))
            .map(|(s, v)| cosine(s, v))
            .unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    members.chunks(max_size).map(|c| c.to_vec()).collect()
}

/// 为 `smallest` 找到最相似的可并入簇下标。
fn best_merge_target(
    smallest: &[usize],
    clusters: &[Cluster],
    candidates: &[Memory],
    embeddings: &HashMap<Uuid, Vec<f32>>,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (ci, cluster) in clusters.iter().enumerate() {
        let sim = avg_cross_similarity(smallest, cluster, candidates, embeddings);
        if best.map(|(_, s)| sim > s).unwrap_or(true) {
            best = Some((ci, sim));
        }
    }
    best.map(|(i, _)| i)
}

/// 两个簇之间的平均交叉相似度（贪心，用于合并决策）。
fn avg_cross_similarity(
    a: &[usize],
    b: &[usize],
    candidates: &[Memory],
    embeddings: &HashMap<Uuid, Vec<f32>>,
) -> f32 {
    let mut sum = 0.0f32;
    let mut n = 0usize;
    for &i in a {
        let Some(vi) = embeddings.get(&candidates[i].id) else {
            continue;
        };
        for &j in b {
            if let Some(vj) = embeddings.get(&candidates[j].id) {
                sum += cosine(vi, vj);
                n += 1;
            }
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f32
    }
}

/// 分簇提炼的 System Prompt（要求 JSON 结构化输出）。
pub const EXTRACT_SYSTEM_PROMPT: &str = "你是一个记忆整合引擎。给定一组同主题的历史记忆，\
请提炼为结构化 JSON，字段如下：\n\
{\"facts\": [{\"text\": \"...\", \"source_hints\": [\"...\"]}], \
\"summary\": \"...\", \"contradictions\": [{\"a\": 0, \"b\": 2, \"reason\": \"...\"}]}\n\
约束：\n\
1. facts 为原子事实，逐条独立、可检索，必须保留原始表述中的关键数据（数字/文件路径/行号）；\n\
2. summary 为跨记忆的关联、规律或行动建议，不超过 500 字；\n\
3. contradictions 仅记录相互矛盾的事实下标对与理由（下标从 0 开始，对应给定记忆顺序）；无矛盾则为空数组；\n\
4. 所有内容必须来自给定记忆，禁止编造；语言与输入一致；\n\
5. 只输出 JSON，不要输出任何解释、Markdown 代码块或额外文字。";

/// 对单个簇做结构化提炼。
///
/// `use_llm=false` 或无 LLM 时，回退为模板 Markdown 摘要（保证鲁棒）。
///
/// # 错误
///
/// 仅当 LLM 真实调用失败（网络/鉴权等）时返回 `Err`，以便调用方中止本轮
/// Dream（避免 LLM 故障时写入半成品）；JSON 解析失败回退模板，不报错。
pub async fn extract_cluster(
    llm: Option<&Arc<dyn LlmClient>>,
    memories: &[Memory],
    use_llm: bool,
) -> Result<ClusterExtraction, String> {
    if use_llm {
        if let Some(llm) = llm {
            let user = build_extract_prompt(memories);
            let messages = vec![
                LlmMessage::system(EXTRACT_SYSTEM_PROMPT),
                LlmMessage::user(user),
            ];
            let raw = llm
                .chat_with_think(&messages, mr_common::ThinkLevel::Off)
                .await
                .map_err(|e| format!("Fusion extraction LLM failed: {}", e))?;
            if let Some(parsed) = parse_extraction(&raw) {
                return Ok(parsed);
            }
            warn!(target: "dream", "Fusion extraction JSON parse failed, falling back");
        }
    }
    Ok(fallback_extraction(memories))
}

/// 构造分簇提炼的用户提示。
fn build_extract_prompt(memories: &[Memory]) -> String {
    let mut s = String::from("待提炼的同主题记忆（按顺序编号，下标从 0 开始）：\n\n");
    for (i, m) in memories.iter().enumerate() {
        s.push_str(&format!(
            "[{}] ({}, tags: [{}], importance: {}) {}\n",
            i,
            m.memory_type,
            m.tags.join(", "),
            m.importance,
            m.content
        ));
    }
    s
}

/// 解析 LLM 返回的 JSON（容忍代码块包裹与前后噪音）。
pub fn parse_extraction(raw: &str) -> Option<ClusterExtraction> {
    let cleaned = strip_code_fence(raw);
    // 直接解析
    if let Ok(x) = serde_json::from_str::<ClusterExtraction>(&cleaned) {
        return Some(x);
    }
    // 截取首个 { 到末个 }
    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<ClusterExtraction>(&cleaned[start..=end]).ok()
}

/// 剥离 ```json ... ``` 包裹。
fn strip_code_fence(raw: &str) -> String {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        let rest = rest.trim_start_matches('\n');
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
        return rest.trim().to_string();
    }
    s.to_string()
}

/// 回退提炼：模板摘要 + 每条记忆作为一条事实。
fn fallback_extraction(memories: &[Memory]) -> ClusterExtraction {
    let facts: Vec<FactItem> = memories
        .iter()
        .map(|m| FactItem {
            text: m.content.clone(),
            source_hints: Vec::new(),
        })
        .collect();
    let summary = format!(
        "[Dream 分簇摘要] 本簇整合了 {} 条记忆。",
        memories.len()
    );
    ClusterExtraction {
        facts,
        summary,
        contradictions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_common::{Memory, MemoryType};

    fn mem_with(content: &str, importance: f32) -> Memory {
        let mut m = Memory::new(content.to_string(), MemoryType::Knowledge);
        m.importance = importance;
        m
    }

    fn emb(v: &[f32]) -> Vec<f32> {
        v.to_vec()
    }

    #[test]
    fn test_cosine_basic() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn test_cluster_by_similarity() {
        let a = mem_with("rust async runtime tokio", 0.9);
        let b = mem_with("rust async runtime tokio performance", 0.8);
        let c = mem_with("cooking pasta recipes", 0.7);
        let candidates = vec![a.clone(), b.clone(), c.clone()];

        let mut embeddings = HashMap::new();
        embeddings.insert(a.id, emb(&[1.0, 0.0]));
        embeddings.insert(b.id, emb(&[0.95, 0.1]));
        embeddings.insert(c.id, emb(&[0.0, 1.0]));

        let clusters = cluster_candidates(&candidates, &embeddings, 0.8, 20, 12);
        // a 与 b 同簇，c 独立
        assert_eq!(clusters.len(), 2);
        let sizes: Vec<usize> = clusters.iter().map(|c| c.len()).collect();
        assert!(sizes.contains(&2));
        assert!(sizes.contains(&1));
    }

    #[test]
    fn test_cluster_fallback_without_vectors() {
        let a = mem_with("a", 0.9);
        let b = mem_with("b", 0.8);
        let candidates = vec![a, b];
        let clusters = cluster_candidates(&candidates, &HashMap::new(), 0.8, 20, 12);
        // 同 project(None) 同类型 → 同组
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].len(), 2);
    }

    #[test]
    fn test_cluster_respects_max_size() {
        let mut candidates = Vec::new();
        let mut embeddings = HashMap::new();
        for i in 0..10 {
            let m = mem_with(&format!("m{}", i), 0.5);
            embeddings.insert(m.id, emb(&[1.0, 0.01 * i as f32]));
            candidates.push(m);
        }
        let clusters = cluster_candidates(&candidates, &embeddings, 0.5, 3, 12);
        assert!(clusters.iter().all(|c| c.len() <= 3));
    }

    #[test]
    fn test_parse_extraction_json() {
        let raw = r#"{"facts":[{"text":"x","source_hints":["a.rs:1"]}],"summary":"s","contradictions":[{"a":0,"b":1,"reason":"r"}]}"#;
        let x = parse_extraction(raw).unwrap();
        assert_eq!(x.facts.len(), 1);
        assert_eq!(x.contradictions.len(), 1);
    }

    #[test]
    fn test_parse_extraction_code_fence() {
        let raw = "```json\n{\"facts\":[],\"summary\":\"s\"}\n```";
        let x = parse_extraction(raw).unwrap();
        assert_eq!(x.summary, "s");
    }

    #[tokio::test]
    async fn test_extract_fallback_without_llm() {
        let m = mem_with("hello world content", 0.5);
        let x = extract_cluster(None, &[m], true).await.unwrap();
        assert_eq!(x.facts.len(), 1);
        assert!(!x.summary.is_empty());
    }
}
