//! # 检索侧运行指标
//!
//! 以原子计数记录检索、预算分配、写入去重与后台冲突审查的运行情况，
//! 由 `stats` 命令读取并换算为比率。
//!
//! ## 口径
//!
//! - 指标自守护进程启动起累计，重启清零；不落盘，避免把观测数据变成第二份真相。
//! - 比率的分母为 0 时记 0.0。
//! - 只读观测：任何指标都不参与检索排序、淘汰或去重决策。

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use mr_protocol::RetrievalStats;

/// 检索侧计数器集合。
#[derive(Debug, Default)]
pub struct RetrievalMetrics {
    search_requests: AtomicU64,
    budget_requests: AtomicU64,
    budget_empty: AtomicU64,
    budget_overflow: AtomicU64,
    budget_results: AtomicU64,
    results_full: AtomicU64,
    results_truncated: AtomicU64,
    results_summary: AtomicU64,
    results_dense_proxy: AtomicU64,
    dedup_checked: AtomicU64,
    dedup_skipped: AtomicU64,
    dedup_updated: AtomicU64,
    dedup_added: AtomicU64,
    review_runs: AtomicU64,
    review_queued: AtomicU64,
    review_skipped: AtomicU64,
    review_keep: AtomicU64,
    review_supersede_old: AtomicU64,
    review_supersede_new: AtomicU64,
    review_failed: AtomicU64,
    review_last_run_at: AtomicI64,
}

impl RetrievalMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    fn inc(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录一次检索请求（不区分是否带预算）。
    pub fn on_search(&self) {
        Self::inc(&self.search_requests);
    }

    /// 记录一次分层检索的结果分布。
    ///
    /// `candidates` 为分配前候选数，`results` 为最终保留条数；二者之差即被丢弃数，
    /// 大于 0 表示预算不足以覆盖全部候选（预算溢出）。
    pub fn on_budget_result(&self, candidates: usize, results: &[(&str, usize)]) {
        Self::inc(&self.budget_requests);
        let kept: usize = results.iter().map(|(_, n)| *n).sum();
        if kept == 0 {
            Self::inc(&self.budget_empty);
        }
        if candidates > kept {
            Self::inc(&self.budget_overflow);
        }
        self.budget_results
            .fetch_add(kept as u64, Ordering::Relaxed);
        for (tier, n) in results {
            let counter = match *tier {
                "full" => &self.results_full,
                "truncated" => &self.results_truncated,
                "summary" => &self.results_summary,
                _ => &self.results_dense_proxy,
            };
            counter.fetch_add(*n as u64, Ordering::Relaxed);
        }
    }

    pub fn on_dedup_skip(&self) {
        Self::inc(&self.dedup_checked);
        Self::inc(&self.dedup_skipped);
    }

    pub fn on_dedup_update(&self) {
        Self::inc(&self.dedup_checked);
        Self::inc(&self.dedup_updated);
    }

    pub fn on_dedup_add(&self) {
        Self::inc(&self.dedup_checked);
        Self::inc(&self.dedup_added);
    }

    /// 记录一轮审查：送审条数、因上限丢弃条数、完成时间。
    pub fn on_review_run(&self, finished_at_secs: i64, selected: u64, skipped: u64) {
        Self::inc(&self.review_runs);
        self.review_queued.fetch_add(selected, Ordering::Relaxed);
        self.review_skipped.fetch_add(skipped, Ordering::Relaxed);
        self.review_last_run_at
            .store(finished_at_secs, Ordering::Relaxed);
    }

    pub fn on_review_keep(&self) {
        Self::inc(&self.review_keep);
    }

    pub fn on_review_supersede_old(&self) {
        Self::inc(&self.review_supersede_old);
    }

    pub fn on_review_supersede_new(&self) {
        Self::inc(&self.review_supersede_new);
    }

    pub fn on_review_failed(&self) {
        Self::inc(&self.review_failed);
    }

    /// 读取当前快照并计算比率。
    pub fn snapshot(&self) -> RetrievalStats {
        let load = |c: &AtomicU64| c.load(Ordering::Relaxed);
        let budget_requests = load(&self.budget_requests);
        let budget_results = load(&self.budget_results);
        let dedup_checked = load(&self.dedup_checked);
        let dedup_skipped = load(&self.dedup_skipped);
        let dedup_updated = load(&self.dedup_updated);
        let non_full = budget_results.saturating_sub(load(&self.results_full));

        RetrievalStats {
            search_requests: load(&self.search_requests),
            budget_requests,
            budget_empty: load(&self.budget_empty),
            budget_overflow: load(&self.budget_overflow),
            budget_results,
            results_full: load(&self.results_full),
            results_truncated: load(&self.results_truncated),
            results_summary: load(&self.results_summary),
            results_dense_proxy: load(&self.results_dense_proxy),
            dedup_checked,
            dedup_skipped,
            dedup_updated,
            dedup_added: load(&self.dedup_added),
            review_runs: load(&self.review_runs),
            review_queued: load(&self.review_queued),
            review_skipped: load(&self.review_skipped),
            review_keep: load(&self.review_keep),
            review_supersede_old: load(&self.review_supersede_old),
            review_supersede_new: load(&self.review_supersede_new),
            review_failed: load(&self.review_failed),
            review_last_run_at: {
                let secs = self.review_last_run_at.load(Ordering::Relaxed);
                if secs > 0 {
                    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
                } else {
                    None
                }
            },
            empty_injection_rate: ratio(load(&self.budget_empty), budget_requests),
            budget_overflow_rate: ratio(load(&self.budget_overflow), budget_requests),
            downgrade_rate: ratio(non_full, budget_results),
            dedup_hit_rate: ratio(dedup_skipped + dedup_updated, dedup_checked),
        }
    }
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rates_zero_when_no_samples() {
        let snapshot = RetrievalMetrics::new().snapshot();
        assert_eq!(snapshot.empty_injection_rate, 0.0);
        assert_eq!(snapshot.budget_overflow_rate, 0.0);
        assert_eq!(snapshot.downgrade_rate, 0.0);
        assert_eq!(snapshot.dedup_hit_rate, 0.0);
    }

    #[test]
    fn test_budget_metrics() {
        let m = RetrievalMetrics::new();
        // 3 候选、保留 2（含降级），有丢弃 → 溢出
        m.on_budget_result(3, &[("full", 1), ("summary", 1)]);
        // 空结果
        m.on_budget_result(0, &[]);
        let s = m.snapshot();
        assert_eq!(s.budget_requests, 2);
        assert_eq!(s.budget_empty, 1);
        assert_eq!(s.budget_overflow, 1);
        assert_eq!(s.budget_results, 2);
        assert_eq!(s.results_full, 1);
        assert_eq!(s.results_summary, 1);
        assert!((s.empty_injection_rate - 0.5).abs() < 1e-9);
        assert!((s.budget_overflow_rate - 0.5).abs() < 1e-9);
        assert!((s.downgrade_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_dedup_metrics() {
        let m = RetrievalMetrics::new();
        m.on_dedup_skip();
        m.on_dedup_update();
        m.on_dedup_add();
        m.on_dedup_add();
        let s = m.snapshot();
        assert_eq!(s.dedup_checked, 4);
        assert_eq!(s.dedup_skipped, 1);
        assert_eq!(s.dedup_updated, 1);
        assert_eq!(s.dedup_added, 2);
        assert!((s.dedup_hit_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_search_and_review_counters() {
        let m = RetrievalMetrics::new();
        m.on_search();
        m.on_search();
        m.on_review_run(1_700_000_000, 3, 1);
        m.on_review_keep();
        m.on_review_supersede_old();
        m.on_review_supersede_new();
        m.on_review_failed();
        let s = m.snapshot();
        assert_eq!(s.search_requests, 2);
        assert_eq!(s.review_runs, 1);
        assert_eq!(s.review_queued, 3);
        assert_eq!(s.review_skipped, 1);
        assert!(s.review_last_run_at.is_some());
        assert_eq!(s.review_keep, 1);
        assert_eq!(s.review_supersede_old, 1);
        assert_eq!(s.review_supersede_new, 1);
        assert_eq!(s.review_failed, 1);
    }
}
