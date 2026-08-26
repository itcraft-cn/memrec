//! # DreamGate 门控检查
//!
//! Dream 触发需依次通过三个门控：
//!
//! 1. Gate 1: `config.enabled` → `DreamGateResult::Disabled`
//! 2. Gate 2: `time_gate` (min_hours) → `DreamGateResult::TooSoon`
//! 3. Gate 3: `session_gate` (min_sessions) → `DreamGateResult::NotEnoughSessions`

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
    NotEnoughSessions {
        current: u32,
        min: u32,
    },
}

pub struct DreamGate;

impl DreamGate {
    /// 检查是否允许执行 Dream。
    ///
    /// 时间门槛基于持久化的 [`DreamState`]（上次执行完成时间），
    /// 与并发锁（`dream.lock`）分离，释放锁后时间信息不丢失。
    pub fn check(
        config: &DreamConfig,
        last_state: Option<&DreamState>,
        current_sessions: u32,
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

            let session_diff = current_sessions.saturating_sub(state.session_count);
            if session_diff < 1 {
                return DreamGateResult::NotEnoughSessions {
                    current: current_sessions,
                    min: state.session_count + 1,
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

    fn make_state(hours_ago: f64, session_count: u32) -> DreamState {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        DreamState {
            last_run_at: now - (hours_ago * 3600.0) as i64,
            session_count,
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
    fn test_gate_not_enough_sessions() {
        let config = make_config(true, 0.0);
        let state = make_state(30.0, 10);
        // 距上次执行已有新会话
        let result = DreamGate::check(&config, Some(&state), 10);
        assert_eq!(
            result,
            DreamGateResult::NotEnoughSessions {
                current: 10,
                min: 11
            }
        );
    }
}
