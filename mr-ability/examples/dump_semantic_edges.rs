//! 临时审计工具：转储指定数据目录中的语义边（evidence 以 `dream:llm-semantic` 开头）。
//!
//! 用法：`cargo run --release --example dump_semantic_edges -- <data_dir>`
//!
//! 注意：该工具以独占方式打开 RocksDB，运行前须停止 memrecd。

use std::sync::Arc;

use mr_ability::storage::{EdgeStorage, EdgeStore, RocksDBStore};
use mr_common::EdgeType;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let data_dir = std::env::args()
        .nth(1)
        .expect("usage: dump_semantic_edges <data_dir>");
    let rocksdb = Arc::new(RocksDBStore::open(std::path::Path::new(&data_dir))?);
    let store = EdgeStore::new(rocksdb);

    let types = [
        EdgeType::Similar,
        EdgeType::References,
        EdgeType::CausedBy,
        EdgeType::Evolves,
        EdgeType::Contradicts,
        EdgeType::SameProject,
    ];
    for t in types {
        let edges = store.list_by_type(t).await?;
        let semantic: Vec<_> = edges
            .iter()
            .filter(|e| e.evidence.starts_with("dream:llm-semantic"))
            .collect();
        println!(
            "{:?}: total={} semantic={}",
            t,
            edges.len(),
            semantic.len()
        );
        for e in semantic.iter().take(20) {
            println!(
                "  {} -> {} w={:.2} | {}",
                &e.source_id.to_string()[..8],
                &e.target_id.to_string()[..8],
                e.weight,
                e.evidence
            );
        }
    }
    Ok(())
}
