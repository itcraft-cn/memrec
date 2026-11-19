//! # 记忆过期清扫
//!
//! “遗忘”的第二条通道：除“新事实覆盖旧事实”（dedup 的 `superseded`）之外，
//! 一次性记忆还需要按时间自然失效。
//!
//! ## 语义
//!
//! - 判据：`Memory::expires_at <= now` 且未被软删；
//! - 动作：调用 [`MemoryStorage::delete`]（软删除，可 `restore`），而非物理删除，
//!   保留审计与误删恢复能力；
//! - 调用方应传入 **索引一致型存储**（`IndexingStorage`），使软删同时摘除
//!   向量与全文索引；传入裸存储会留下“幽灵命中”。
//!
//! ## 为什么不用检索期过滤
//!
//! 检索期过滤需要在每次查询时判断 `expires_at`（索引载荷不含该字段，需回表），
//! 且过期条目仍占用候选池。清扫把成本摊到固定周期，读路径保持简单。

use chrono::{DateTime, Utc};

use crate::storage::MemoryStorage;

/// 单次清扫返回的统计。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    /// 扫描到的记忆总数
    pub scanned: usize,
    /// 已过期并软删的记忆数
    pub swept: usize,
    /// 软删失败的记忆数（不影响其余条目）
    pub failed: usize,
}

/// 扫描并软删所有已过期记忆。
///
/// `now` 由调用方传入，便于测试注入固定时间；生产侧传 `Utc::now()`。
///
/// 取数走 `list_created_between(0, now, MAX)`，依赖按创建时间的二级索引，
/// 避免全量加载已软删记录。单条失败仅记录并继续，不中断整轮清扫。
pub async fn sweep_expired(
    storage: &dyn MemoryStorage,
    now: DateTime<Utc>,
) -> anyhow::Result<SweepReport> {
    // 区间语义为 `(after, before]` 且精度为秒；上界 +1 秒以覆盖“本秒内创建”
    // 的记录，否则同一秒写入的过期记忆要等到下一轮才会被扫到。
    let memories = storage
        .list_created_between(0, now.timestamp().saturating_add(1), usize::MAX)
        .await?;

    let mut report = SweepReport {
        scanned: memories.len(),
        ..Default::default()
    };

    for memory in memories {
        if memory.is_deleted || !memory.is_expired(now) {
            continue;
        }
        match storage.delete(&memory.id).await {
            Ok(_) => {
                report.swept += 1;
                tracing::debug!(
                    target: "expiry",
                    "expired memory soft-deleted: {} (expires_at={:?})",
                    memory.id,
                    memory.expires_at
                );
            }
            Err(e) => {
                report.failed += 1;
                tracing::warn!(
                    target: "expiry",
                    "failed to soft-delete expired memory {}: {:#}",
                    memory.id,
                    e
                );
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStore, RocksDBStore};
    use mr_common::{Memory, MemoryType};
    use std::sync::Arc;

    async fn store() -> (Arc<MemoryStore>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(RocksDBStore::open(dir.path()).unwrap());
        (Arc::new(MemoryStore::new(db)), dir)
    }

    #[tokio::test]
    async fn test_sweep_soft_deletes_only_expired() {
        let (store, _dir) = store().await;
        let now = Utc::now();

        let expired = Memory::new("临时上下文".to_string(), MemoryType::Context)
            .with_expires_at(Some(now - chrono::Duration::minutes(1)));
        let future = Memory::new("未到期".to_string(), MemoryType::Knowledge)
            .with_expires_at(Some(now + chrono::Duration::hours(1)));
        let permanent = Memory::new("永久".to_string(), MemoryType::Knowledge);

        store.save(&expired).await.unwrap();
        store.save(&future).await.unwrap();
        store.save(&permanent).await.unwrap();

        let report = sweep_expired(store.as_ref(), now).await.unwrap();
        assert_eq!(report.swept, 1);
        assert_eq!(report.failed, 0);

        assert!(store.get(&expired.id).await.unwrap().unwrap().is_deleted);
        assert!(!store.get(&future.id).await.unwrap().unwrap().is_deleted);
        assert!(!store.get(&permanent.id).await.unwrap().unwrap().is_deleted);
    }

    #[tokio::test]
    async fn test_sweep_is_idempotent() {
        let (store, _dir) = store().await;
        let now = Utc::now();
        let expired = Memory::new("一次性".to_string(), MemoryType::Context)
            .with_expires_at(Some(now - chrono::Duration::seconds(1)));
        store.save(&expired).await.unwrap();

        let first = sweep_expired(store.as_ref(), now).await.unwrap();
        assert_eq!(first.swept, 1);
        let second = sweep_expired(store.as_ref(), now).await.unwrap();
        assert_eq!(second.swept, 0, "已软删的过期记忆不应重复计数");
    }
}
