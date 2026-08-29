//! # DreamGate 门控检查
//!
//! Dream 触发需依次通过三个门控：
//!
//! 1. Gate 1: `config.enabled` → `DreamGateResult::Disabled`
//! 2. Gate 2: `time_gate` (min_hours_between) → `DreamGateResult::TooSoon`
//! 3. Gate 3: `new_memories_gate` (min_new_memories) → `DreamGateResult::NotEnoughNewMemories`
//!
//! Gate 3 采用"新记忆写入"信号：距上次执行后**用户新增记忆数**不足
//! `min_new_memories` 时拦截，避免在没有新数据产生的情况下 Dream 空转。
//! 新增数由调用方统计（`created_at > last_run_at` 且非 System 来源），
//! 不使用记忆总数相减（净增量会被用户删/加抵消而误拦截）。

use std::time::{SystemTime, UNIX_EPOCH};

use mr_common::types::DreamConfig;

use super::lock::DreamState;

#[derive(Debug, Clone, PartialEq)]
pub enum DreamGateResult {
    Allowed,
    Disabled,
    /// Dream 需要 LLM 但未接入（`requires_llm` 且无 LLM 客户端）
    LlmNotConfigured,
    TooSoon {
        hours_since_last: f64,
        min_hours: f64,
    },
    NotEnoughNewMemories {
        new_memories: usize,
        min_new_memories: usize,
    },
}

pub struct DreamGate;

impl DreamGate {
    /// 检查是否允许执行 Dream。
    ///
    /// 时间门槛基于持久化的 [`DreamState`]（上次执行完成时间），
    /// 与并发锁（`dream.lock`）分离，释放锁后时间信息不丢失。
    /// 新记忆门槛基于 `new_user_memories`（距上次执行后的用户新增记忆数，
    /// 由调用方按 `created_at > last_run_at` 且非 System 来源统计）。
    pub fn check(
        config: &DreamConfig,
        last_state: Option<&DreamState>,
        new_user_memories: usize,
    ) -> DreamGateResult {
        if !config.enabled {
            return DreamGateResult::Disabled;
        }

        if let Some(state) = last_state {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let hours_since_last = (now - state.last_run_at) as f64 / 3600.0;

            if hours_since_last < config.min_hours_between {
                return DreamGateResult::TooSoon {
                    hours_since_last,
                    min_hours: config.min_hours_between,
                };
            }

            // 新记忆门槛：距上次执行后用户新增记忆数需达到 min_new_memories
            if new_user_memories < config.min_new_memories {
                return DreamGateResult::NotEnoughNewMemories {
                    new_memories: new_user_memories,
                    min_new_memories: config.min_new_memories,
                };
            }
        }

        DreamGateResult::Allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(enabled: bool, min_hours: f64) -> DreamConfig {
        DreamConfig {
            enabled,
            min_hours_between: min_hours,
            ..Default::default()
        }
    }

    fn make_state(hours_ago: f64, memory_count: u32) -> DreamState {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        DreamState {
            last_run_at: now - (hours_ago * 3600.0) as i64,
            memory_count,
        }
    }

    #[test]
    fn test_gate_disabled() {
        let config = make_config(false, 24.0);
        let result = DreamGate::check(&config, None, 10);

        assert_eq!(result, DreamGateResult::Disabled);
    }

    #[test]
    fn test_gate_allowed_no_state() {
        let config = make_config(true, 24.0);
        let result = DreamGate::check(&config, None, 10);

        assert_eq!(result, DreamGateResult::Allowed);
    }

    #[test]
    fn test_gate_too_soon() {
        let config = make_config(true, 24.0);
        let state = make_state(1.0, 5);
        let result = DreamGate::check(&config, Some(&state), 10);

        assert!(matches!(result, DreamGateResult::TooSoon { .. }));
    }

    #[test]
    fn test_gate_allowed_after_time() {
        let config = make_config(true, 24.0);
        let state = make_state(30.0, 5);
        let result = DreamGate::check(&config, Some(&state), 10);

        assert_eq!(result, DreamGateResult::Allowed);
    }

    #[test]
    fn test_gate_not_enough_new_memories() {
        let config = make_config(true, 0.0);
        // 距上次执行已超过时间门槛，但用户新增记忆不足 min_new_memories（默认 1）
        let state = make_state(30.0, 10);
        let result = DreamGate::check(&config, Some(&state), 0);
        assert_eq!(
            result,
            DreamGateResult::NotEnoughNewMemories {
                new_memories: 0,
                min_new_memories: 1
            }
        );
    }

    #[test]
    fn test_gate_allowed_with_new_memories() {
        let config = make_config(true, 0.0);
        // 距上次执行新增 2 条用户记忆，达到默认 min_new_memories = 1
        let state = make_state(30.0, 10);
        let result = DreamGate::check(&config, Some(&state), 2);

        assert_eq!(result, DreamGateResult::Allowed);
    }

    #[test]
    fn test_gate_custom_min_new_memories() {
        let config = DreamConfig {
            enabled: true,
            min_hours_between: 0.0,
            min_new_memories: 3,
            ..Default::default()
        };
        let state = make_state(30.0, 10);

        // 新增 2 条 < 3，拦截
        let result = DreamGate::check(&config, Some(&state), 2);
        assert!(matches!(
            result,
            DreamGateResult::NotEnoughNewMemories { .. }
        ));

        // 新增 3 条 >= 3，放行
        let result = DreamGate::check(&config, Some(&state), 3);
        assert_eq!(result, DreamGateResult::Allowed);
    }
}
