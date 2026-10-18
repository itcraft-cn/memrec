//! # 离线检索评测（Eval Harness）
//!
//! 零侵入的端到端标尺：以 JSONL 数据集驱动**只读**检索请求，度量检索命中率
//! 与排序质量，用于验证 Dream 重构（Stage F/N、检索图扩散）带来的召回增益。
//!
//! ## 数据集格式
//!
//! 每行一个 JSON 对象：
//!
//! ```json
//! {"query": "用户查询", "relevant": ["<memory-uuid>", "..."]}
//! ```
//!
//! `relevant` 为该查询的**相关记忆 ID**（须已存在于记忆库）。行内可含
//! `"id"`（字符串，仅用于报告定位，可省略）。
//!
//! ## 度量
//!
//! - `hit@k`：前 k 命中是否包含任一相关记忆；
//! - `recall@k`：前 k 命中覆盖相关记忆的比例；
//! - `mrr`：首个相关命中的倒数排名；
//! - `ndcg@k`：归一化折损累计增益。
//!
//! ## 设计要点
//!
//! - **只读**：仅发送 `SearchMemory` 请求，不写入、不改动服务端状态；
//! - **与运行期度量互补**：此处为离线端到端标尺，`DreamRun.metrics` 为运行期内在指标。

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use mr_protocol::{
    JsonRpcRequest, RequestAction, RequestParams, ResponseResult, SearchMemoryParams,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client::Client;

/// 单条评测用例。
#[derive(Debug, Clone, Deserialize)]
pub struct EvalCase {
    /// 查询文本
    pub query: String,
    /// 相关记忆 ID 列表
    #[serde(default)]
    pub relevant: Vec<Uuid>,
    /// 用例标识（可选，便于报告定位）
    #[serde(default)]
    pub id: Option<String>,
}

/// 单条用例的评测结果。
#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub id: Option<String>,
    pub query: String,
    pub retrieved: usize,
    /// 该用例的相关记忆数（0 表示无标注，不参与比例类指标）
    pub relevant_count: usize,
    pub hit_at_k: bool,
    pub recall_at_k: f32,
    pub reciprocal_rank: f32,
    pub ndcg_at_k: f32,
}

/// 汇总报告。
#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub cases: usize,
    pub k: usize,
    pub hit_rate: f32,
    pub mean_recall: f32,
    pub mrr: f32,
    pub mean_ndcg: f32,
    pub per_case: Vec<CaseResult>,
}

/// 从 JSONL 文件加载用例（跳过空行与以 `#` 开头的注释行）。
pub fn load_cases(path: &Path) -> Result<Vec<EvalCase>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读取评测集失败: {}", path.display()))?;
    let mut cases = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let case: EvalCase = serde_json::from_str(line)
            .with_context(|| format!("解析第 {} 行失败: {}", i + 1, line))?;
        cases.push(case);
    }
    Ok(cases)
}

/// 计算单条用例指标。
///
/// - `ranked`：检索返回的记忆 ID（按相关度降序，可多于 k）
/// - `relevant`：相关记忆 ID 集合
/// - `k`：截断位置
pub fn score_case(ranked: &[Uuid], relevant: &[Uuid], k: usize) -> (bool, f32, f32, f32) {
    if relevant.is_empty() {
        return (false, 0.0, 0.0, 0.0);
    }
    let topk = &ranked[..ranked.len().min(k)];
    let rel_set: HashSet<Uuid> = relevant.iter().copied().collect();

    let hits = topk.iter().filter(|id| rel_set.contains(id)).count();
    let hit = hits > 0;
    let recall = hits as f32 / relevant.len() as f32;

    let rr = topk
        .iter()
        .position(|id| rel_set.contains(id))
        .map(|pos| 1.0 / (pos as f32 + 1.0))
        .unwrap_or(0.0);

    // nDCG：相关度二值，理想排序为全部相关记忆置顶
    let dcg: f32 = topk
        .iter()
        .enumerate()
        .filter(|(_, id)| rel_set.contains(id))
        .map(|(i, _)| 1.0 / ((i as f32 + 2.0).log2()))
        .sum();
    let ideal_len = relevant.len().min(k);
    let idcg: f32 = (0..ideal_len)
        .map(|i| 1.0 / ((i as f32 + 2.0).log2()))
        .sum();
    let ndcg = if idcg > 0.0 { dcg / idcg } else { 0.0 };

    (hit, recall, rr, ndcg)
}

/// 运行评测：对每个用例发送只读检索请求并汇总指标。
///
/// - `alpha`：向量检索权重（0.0 纯全文，1.0 纯向量），透传给 daemon。
/// - `mmr_enabled`：是否启用 MMR 多样性重排；关闭可隔离验证纯相关性排序。
pub async fn run(
    client: &Client,
    cases: &[EvalCase],
    k: usize,
    alpha: f64,
    mmr_enabled: bool,
    mut on_case: impl FnMut(&CaseResult),
) -> Result<EvalReport> {
    let mut per_case = Vec::with_capacity(cases.len());
    for (idx, case) in cases.iter().enumerate() {
        let req = JsonRpcRequest::new(
            RequestAction::SearchMemory,
            Some(RequestParams::SearchMemory(SearchMemoryParams {
                query: case.query.clone(),
                project_id: None,
                include_global: true,
                project_only: false,
                global_only: false,
                cross_project: true,
                memory_type: None,
                top_k: k,
                min_score: 0.0,
                working_dir: None,
                hybrid_alpha: alpha,
                mmr_enabled,
                mmr_lambda: mr_protocol::default_mmr_lambda(),
            })),
            (idx + 1) as u64,
        );

        let response = client.send(&req).await?;
        let ranked: Vec<Uuid> = match response.result {
            Some(ResponseResult::SemanticSearchResult(r)) => {
                r.results.into_iter().map(|h| h.memory_id).collect()
            }
            _ => Vec::new(),
        };

        let (hit, recall, rr, ndcg) = score_case(&ranked, &case.relevant, k);
        let result = CaseResult {
            id: case.id.clone(),
            query: case.query.clone(),
            retrieved: ranked.len(),
            relevant_count: case.relevant.len(),
            hit_at_k: hit,
            recall_at_k: recall,
            reciprocal_rank: rr,
            ndcg_at_k: ndcg,
        };
        on_case(&result);
        per_case.push(result);
    }

    Ok(summarize(per_case, k))
}

/// 汇总各用例指标（无标注 `relevant` 的用例不参与比例类指标）。
pub fn summarize(per_case: Vec<CaseResult>, k: usize) -> EvalReport {
    let cases = per_case.len();
    // 仅统计有标注的用例
    let scored: Vec<&CaseResult> = per_case.iter().filter(|c| c.relevant_count > 0).collect();
    let n = scored.len().max(1);

    let hit_rate = scored.iter().filter(|c| c.hit_at_k).count() as f32 / n as f32;
    let mean_recall = scored.iter().map(|c| c.recall_at_k).sum::<f32>() / n as f32;
    let mrr = scored.iter().map(|c| c.reciprocal_rank).sum::<f32>() / n as f32;
    let mean_ndcg = scored.iter().map(|c| c.ndcg_at_k).sum::<f32>() / n as f32;

    EvalReport {
        cases,
        k,
        hit_rate,
        mean_recall,
        mrr,
        mean_ndcg,
        per_case,
    }
}

/// CLI 入口：加载数据集、运行只读评测并输出报告。
pub async fn eval_retrieval(
    client: &Client,
    dataset: &Path,
    k: usize,
    alpha: f64,
    mmr_enabled: bool,
    human: bool,
) -> Result<()> {
    let cases = load_cases(dataset)?;
    if cases.is_empty() {
        anyhow::bail!("评测集为空: {}", dataset.display());
    }

    let report = run(client, &cases, k, alpha, mmr_enabled, |c| {
        if human {
            let tag = c.id.as_deref().unwrap_or("-");
            eprintln!(
                "[{}] hit@{}={} recall={:.2} rr={:.2} ndcg={:.2}  {}",
                tag,
                k,
                if c.hit_at_k { "Y" } else { "N" },
                c.recall_at_k,
                c.reciprocal_rank,
                c.ndcg_at_k,
                c.query
            );
        }
    })
    .await?;

    if human {
        eprintln!(
            "\n用例 {} 条，k={}\nhit_rate={:.4}  mean_recall={:.4}  mrr={:.4}  mean_ndcg={:.4}",
            report.cases,
            report.k,
            report.hit_rate,
            report.mean_recall,
            report.mrr,
            report.mean_ndcg
        );
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn test_score_case_perfect() {
        let ranked = vec![uid(1), uid(2), uid(3)];
        let relevant = vec![uid(1), uid(2)];
        let (hit, recall, rr, ndcg) = score_case(&ranked, &relevant, 10);
        assert!(hit);
        assert_eq!(recall, 1.0);
        assert_eq!(rr, 1.0);
        assert!((ndcg - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_score_case_partial_and_rank() {
        let ranked = vec![uid(9), uid(1), uid(8)];
        let relevant = vec![uid(1), uid(2)];
        let (hit, recall, rr, ndcg) = score_case(&ranked, &relevant, 10);
        assert!(hit);
        assert_eq!(recall, 0.5);
        assert_eq!(rr, 0.5);
        assert!(ndcg > 0.0 && ndcg < 1.0);
    }

    #[test]
    fn test_score_case_miss() {
        let ranked = vec![uid(5)];
        let relevant = vec![uid(1)];
        let (hit, recall, rr, ndcg) = score_case(&ranked, &relevant, 10);
        assert!(!hit);
        assert_eq!(recall, 0.0);
        assert_eq!(rr, 0.0);
        assert_eq!(ndcg, 0.0);
    }

    #[test]
    fn test_score_case_truncation() {
        // 相关项在第 3 位，k=2 时不命中
        let ranked = vec![uid(9), uid(8), uid(1)];
        let relevant = vec![uid(1)];
        let (hit, recall, _, _) = score_case(&ranked, &relevant, 2);
        assert!(!hit);
        assert_eq!(recall, 0.0);
    }

    #[test]
    fn test_load_cases_skips_comments() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("eval.jsonl");
        std::fs::write(
            &p,
            "# comment\n\n{\"query\":\"q1\",\"relevant\":[\"00000000-0000-0000-0000-000000000001\"]}\n",
        )
        .unwrap();
        let cases = load_cases(&p).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].query, "q1");
        assert_eq!(cases[0].relevant.len(), 1);
    }
}
