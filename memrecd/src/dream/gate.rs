//! # DreamGate 门控检查
//!
//! Dream 触发需依次通过三个门控：
//!
//! 1. Gate 1: `config.enabled` → `DreamGateResult::Disabled`
//! 2. Gate 2: `time_gate` (min_hours) → `DreamGateResult::TooSoon`
//! 3. Gate 3: `session_gate` (min_sessions) → `DreamGateResult::NotEnoughSessions`

use std::time::{SystemTime, UNIX_EPOCH};

use memrec_common::types::DreamConfig;

use super::lock::LockContent;

#[derive(Debug, Clone, PartialEq)]
pub enum DreamGateResult {
    Allowed,
    Disabled,
    TooSoon { hours_since_last: f64, min_hours: f64 },
    NotEnoughSessions { current: u32, min: u32 },
}

pub struct DreamGate;

impl DreamGate {
    /// 检查是否允许执行 Dream。
    pub fn check(
        config: &DreamConfig,
        last_lock: Option<&LockContent>,
        current_sessions: u32,
    ) -> DreamGateResult {
        if !config.enabled {
            return DreamGateResult::Disabled;
        }

        if let Some(lock) = last_lock {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let hours_since_last = (now - lock.timestamp) as f64 / 3600.0;

            if hours_since_last < config.min_hours_between {
                return DreamGateResult::TooSoon {
                    hours_since_last,
                    min_hours: config.min_hours_between,
                };
            }

            let session_diff = current_sessions.saturating_sub(lock.session_count);
            if session_diff < 1 {
                return DreamGateResult::NotEnoughSessions {
                    current: current_sessions,
                    min: lock.session_count + 1,
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

    fn make_lock(hours_ago: f64, session_count: u32) -> LockContent {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        LockContent {
            pid: 12345,
            timestamp: now - (hours_ago * 3600.0) as i64,
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
    fn test_gate_allowed_no_lock() {
        let config = make_config(true, 24.0);
        let result = DreamGate::check(&config, None, 10);

        assert_eq!(result, DreamGateResult::Allowed);
    }

    #[test]
    fn test_gate_too_soon() {
        let config = make_config(true, 24.0);
        let lock = make_lock(1.0, 5);
        let result = DreamGate::check(&config, Some(&lock), 10);

        assert!(matches!(result, DreamGateResult::TooSoon { .. }));
    }

    #[test]
    fn test_gate_allowed_after_time() {
        let config = make_config(true, 24.0);
        let lock = make_lock(30.0, 5);
        let result = DreamGate::check(&config, Some(&lock), 10);

        assert_eq!(result, DreamGateResult::Allowed);
    }
}