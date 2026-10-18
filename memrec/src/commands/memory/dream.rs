//! Dream 整合与审计命令。

use super::*;

/// Dream 记忆整合命令。
pub async fn dream(client: &Client, force: bool, dry_run: bool, human: bool) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::Dream,
        Some(RequestParams::Dream(DreamParams { force, dry_run })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        if let Some(result) = response.result {
            match result {
                ResponseResult::Dream(d) => {
                    if d.dry_run {
                        println!("Dream dry-run (no changes written):");
                    } else {
                        println!("Dream completed:");
                    }
                    println!("  Integrated: {} memories", d.integrated_count);
                    println!("  Clusters: {}", d.cluster_count);
                    println!("  Edges: {}", d.edge_count);
                    println!("  Propagations: {}", d.propagation_count);
                    println!("  Contradictions: {}", d.contradiction_count);
                    if let Some(id) = d.created_memory_id {
                        println!("  Created memory: {}", id);
                    }
                    if let Some(rid) = d.run_id {
                        println!("  Run: {}", rid);
                    }
                    println!(
                        "  Metrics: clue_retention={:.2} compression={:.2} avg_cluster={:.1}",
                        d.metrics.clue_retention, d.metrics.compression_ratio, d.metrics.avg_cluster_size
                    );
                    println!("  Summary: {}", d.summary);
                }
                _ => println!("Unexpected response type"),
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}

/// Dream 运行审计报告命令。
///
/// 返回最近若干次 DreamRun（含门控结果、产出指纹、边/传播活跃度、LLM 用量与质量度量）。
pub async fn dream_report(client: &Client, last: usize, human: bool) -> Result<()> {
    let request = JsonRpcRequest::new(
        RequestAction::DreamReport,
        Some(RequestParams::DreamReport(DreamReportParams {
            last: Some(last),
            offset: None,
        })),
        1,
    );

    let response = client.send(&request).await?;

    if human {
        if let Some(result) = response.result {
            match result {
                ResponseResult::DreamReport(r) => {
                    println!("Dream runs: {} (total {})", r.runs.len(), r.total);
                    for run in &r.runs {
                        println!(
                            "  [{}] {} candidates={} clusters={} edges={} propagations={} status={:?} gate={:?}",
                            run.started_at.format("%Y-%m-%d %H:%M:%S"),
                            run.run_id,
                            run.candidate_count,
                            run.cluster_count,
                            run.edge_count,
                            run.propagation_count,
                            run.status,
                            run.gate_outcome
                        );
                        if let Some(err) = &run.error {
                            println!("      error: {}", err);
                        }
                    }
                }
                _ => println!("Unexpected response type"),
            }
        } else if let Some(err) = response.error {
            println!("Error: {}", err.message);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }

    Ok(())
}
