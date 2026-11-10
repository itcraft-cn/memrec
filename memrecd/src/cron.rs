//! # 最小 cron 表达式解析与下次执行时间计算
//!
//! 支持标准 5 段式 cron（分 时 日 月 周），用于周级/自定义周期的后台任务调度。
//!
//! ## 支持语法
//!
//! - `*` / `?`：任意值
//! - `a`、`a-b`：单值或闭区间
//! - `*/n`、`a-b/n`、`a/n`：步长
//! - 逗号列表：`1,3,5`
//! - 月份/星期英文缩写：`jan`–`dec`、`sun`–`sat`
//! - 星期 `0` 与 `7` 均表示周日
//!
//! ## 语义
//!
//! - 日（DOM）与周（DOW）同时受限时，按标准 cron 语义取“或”：任一命中即执行；
//! - 计算基于本地时区，严格返回 `now` 之后的时刻；
//! - 400 天内无匹配（如 `0 0 29 2 *` 跨闰年）返回 `None`，调用方按“漏跑”处理。
//!
//! 未实现秒级字段、`L`/`W`/`#` 等扩展语法。

use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike, Weekday};

/// 解析后的 cron 调度器。
#[derive(Debug, Clone)]
pub struct CronSchedule {
    minutes: u64,
    hours: u64,
    days_of_month: u64,
    months: u64,
    days_of_week: u64,
    /// DOM 是否为 `*`（用于 DOM/DOW 的“或”语义判定）
    dom_any: bool,
    /// DOW 是否为 `*`
    dow_any: bool,
}

/// cron 解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronError(pub String);

impl std::fmt::Display for CronError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CronError {}

impl CronSchedule {
    /// 解析 5 段式 cron 表达式。
    pub fn parse(expr: &str) -> Result<Self, CronError> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(CronError(format!(
                "cron 需要 5 段（分 时 日 月 周），实际 {} 段: {:?}",
                fields.len(),
                expr
            )));
        }

        let minutes = parse_field(fields[0], 0, 59, &[], "minute")?;
        let hours = parse_field(fields[1], 0, 23, &[], "hour")?;
        let days_of_month = parse_field(fields[2], 1, 31, &[], "day-of-month")?;
        let months = parse_field(fields[3], 1, 12, &MONTH_NAMES, "month")?;
        // 星期字段允许 0-7，其中 7 在 parse_field 内归一为 0（周日）。
        let days_of_week = parse_field(fields[4], 0, 7, &DAY_NAMES, "day-of-week")?;

        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
            dom_any: fields[2] == "*" || fields[2] == "?",
            dow_any: fields[4] == "*" || fields[4] == "?",
        })
    }

    /// 返回严格晚于 `now` 的下一次执行时刻。
    pub fn next_after(&self, now: DateTime<Local>) -> Option<DateTime<Local>> {
        let mut t = (now + Duration::minutes(1))
            .with_second(0)?
            .with_nanosecond(0)?;

        // 400 天足以覆盖闰年；每天最多 1440 次分钟检查，但日/时不匹配时会整段跳过。
        for _ in 0..(400 * 24 * 60) {
            if !self.has_month(t.month()) {
                t = next_day_start(t)?;
                continue;
            }
            if !self.matches_day(t) {
                t = next_day_start(t)?;
                continue;
            }
            if !has_bit(self.hours, t.hour()) {
                t = next_hour(t)?;
                continue;
            }
            if !has_bit(self.minutes, t.minute()) {
                t += Duration::minutes(1);
                continue;
            }
            return Some(t);
        }
        None
    }

    fn has_month(&self, month: u32) -> bool {
        has_bit(self.months, month)
    }

    fn matches_day(&self, t: DateTime<Local>) -> bool {
        let dom_ok = has_bit(self.days_of_month, t.day());
        let dow_ok = has_bit(self.days_of_week, weekday_num(t.weekday()));
        match (self.dom_any, self.dow_any) {
            (true, true) => true,
            (true, false) => dow_ok,
            (false, true) => dom_ok,
            // 标准 cron：DOM 与 DOW 同时受限时取“或”
            (false, false) => dom_ok || dow_ok,
        }
    }
}

fn next_day_start(t: DateTime<Local>) -> Option<DateTime<Local>> {
    let next = t.date_naive().succ_opt()?;
    Local
        .from_local_datetime(&next.and_hms_opt(0, 0, 0)?)
        .earliest()
}

fn next_hour(t: DateTime<Local>) -> Option<DateTime<Local>> {
    let next = (t + Duration::hours(1))
        .with_minute(0)?
        .with_second(0)?
        .with_nanosecond(0)?;
    Some(next)
}

fn weekday_num(day: Weekday) -> u32 {
    day.num_days_from_sunday()
}

fn has_bit(mask: u64, value: u32) -> bool {
    value < 64 && (mask & (1u64 << value)) != 0
}

const MONTH_NAMES: [(&str, u32); 12] = [
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("may", 5),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
];

const DAY_NAMES: [(&str, u32); 7] = [
    ("sun", 0),
    ("mon", 1),
    ("tue", 2),
    ("wed", 3),
    ("thu", 4),
    ("fri", 5),
    ("sat", 6),
];

/// 解析单个字段为位掩码。
fn parse_field(
    spec: &str,
    min: u32,
    max: u32,
    names: &[(&str, u32)],
    label: &str,
) -> Result<u64, CronError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(CronError(format!("{label} 字段为空")));
    }
    if spec == "*" || spec == "?" {
        return Ok(range_mask(min, max));
    }

    let mut mask = 0u64;
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(CronError(format!("{label} 字段存在空项: {spec}")));
        }
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step
                    .parse()
                    .map_err(|_| CronError(format!("{label} 步长非法: {part}")))?;
                if step == 0 {
                    return Err(CronError(format!("{label} 步长不能为 0: {part}")));
                }
                (range, step)
            }
            None => (part, 1),
        };

        let (start, end) = if range == "*" || range == "?" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                parse_value(a, min, max, names, label)?,
                parse_value(b, min, max, names, label)?,
            )
        } else {
            let value = parse_value(range, min, max, names, label)?;
            // `a/n` 语义：从 a 到上限，步长 n
            if part.contains('/') {
                (value, max)
            } else {
                (value, value)
            }
        };

        if start > end {
            return Err(CronError(format!("{label} 区间倒置: {part}")));
        }
        let mut value = start;
        while value <= end {
            // 星期 7 归一为 0（周日）
            let v = if label == "day-of-week" && value == 7 {
                0
            } else {
                value
            };
            mask |= 1u64 << v;
            value += step;
        }
    }
    Ok(mask)
}

fn parse_value(
    raw: &str,
    min: u32,
    max: u32,
    names: &[(&str, u32)],
    label: &str,
) -> Result<u32, CronError> {
    let raw = raw.trim().to_ascii_lowercase();
    let value = if let Some((_, v)) = names.iter().find(|(name, _)| *name == raw) {
        *v
    } else {
        raw.parse::<u32>()
            .map_err(|_| CronError(format!("{label} 值非法: {raw}")))?
    };
    if value < min || value > max {
        return Err(CronError(format!(
            "{label} 超出范围 [{min}, {max}]: {value}"
        )));
    }
    Ok(value)
}

fn range_mask(min: u32, max: u32) -> u64 {
    let mut mask = 0u64;
    for v in min..=max {
        mask |= 1u64 << v;
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap()
    }

    #[test]
    fn test_parse_errors() {
        assert!(CronSchedule::parse("* * * *").is_err());
        assert!(CronSchedule::parse("60 * * * *").is_err());
        assert!(CronSchedule::parse("*/0 * * * *").is_err());
        assert!(CronSchedule::parse("5-1 * * * *").is_err());
        assert!(CronSchedule::parse("x * * * *").is_err());
    }

    #[test]
    fn test_weekly_saturday_0400() {
        let cron = CronSchedule::parse("0 4 * * 6").unwrap();
        // 2026-11-13 是周五
        let next = cron.next_after(local(2026, 11, 13, 12, 0)).unwrap();
        assert_eq!(next, local(2026, 11, 14, 4, 0));
        // 周六 03:59 仍是当天
        let next = cron.next_after(local(2026, 11, 14, 3, 59)).unwrap();
        assert_eq!(next, local(2026, 11, 14, 4, 0));
        // 周六 04:00 之后顺延一周
        let next = cron.next_after(local(2026, 11, 14, 4, 0)).unwrap();
        assert_eq!(next, local(2026, 11, 21, 4, 0));
    }

    #[test]
    fn test_step_and_range() {
        let cron = CronSchedule::parse("*/15 9-18 * * 1-5").unwrap();
        let next = cron.next_after(local(2026, 11, 16, 9, 7)).unwrap();
        assert_eq!(next, local(2026, 11, 16, 9, 15));
        // 周五 18:45（当天最后一个槽位）之后跳到周一 09:00
        let next = cron.next_after(local(2026, 11, 20, 18, 45)).unwrap();
        assert_eq!(next, local(2026, 11, 23, 9, 0));
    }

    #[test]
    fn test_names_and_lists() {
        let cron = CronSchedule::parse("0 4 * * sat,sun").unwrap();
        let next = cron.next_after(local(2026, 11, 13, 12, 0)).unwrap();
        assert_eq!(next, local(2026, 11, 14, 4, 0));

        let cron = CronSchedule::parse("30 2 1 jan *").unwrap();
        let next = cron.next_after(local(2026, 11, 13, 12, 0)).unwrap();
        assert_eq!(next, local(2027, 1, 1, 2, 30));
    }

    #[test]
    fn test_dom_dow_or_semantics() {
        // DOM=1 或 DOW=周日
        let cron = CronSchedule::parse("0 4 1 * 0").unwrap();
        // 2026-11-15 是周日
        let next = cron.next_after(local(2026, 11, 13, 12, 0)).unwrap();
        assert_eq!(next, local(2026, 11, 15, 4, 0));
    }

    #[test]
    fn test_sunday_alias_seven() {
        let cron = CronSchedule::parse("0 4 * * 7").unwrap();
        let next = cron.next_after(local(2026, 11, 13, 12, 0)).unwrap();
        assert_eq!(next, local(2026, 11, 15, 4, 0));
    }

    #[test]
    fn test_question_mark_is_any() {
        let cron = CronSchedule::parse("0 4 ? * ?").unwrap();
        assert!(cron.next_after(local(2026, 11, 13, 12, 0)).is_some());
    }
}
