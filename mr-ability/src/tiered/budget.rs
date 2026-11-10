//! # Token 预算控制
//!
//! 在检索结果上做两层约束：
//!
//! 1. **总预算**：所有结果的 token 估算值之和不得超过 `max_tokens`；
//! 2. **类型优先级**：按 [`TypeBudgetPolicy`] 的顺序与上限分配预算，低优先级
//!    （未列入策略）类型只能使用剩余额度。
//!
//! 超预算时的处理顺序为“先降级表示、后丢弃”：`Full → Truncated → Summary → DenseProxy`。
//!
//! ## Token 估算口径
//!
//! 中英文混排下不能沿用“字符数 × 0.4”的英文启发式（4 字符 ≈ 1 token）：
//! 中文约为 1 字 1 token，按 0.4 估算会低估 2 倍以上，导致 4000 预算实际放行
//! 近万字中文。这里按 CJK 字符 1 token、其他非空白字符 0.25 token、空白 0.1 token
//! 估算，对英文略保守（约 1.3 倍），对中文接近实际。

use mr_common::{MemoryType, RepresentationTier, TypeBudgetPolicy};
use uuid::Uuid;

/// 参与预算分配的候选条目。
///
/// `full_tokens` 为完整内容的 token 估算值；其余层级的估算值由
/// [`BudgetItem::tokens_at`] 推导，保证分配阶段无需渲染内容。
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetItem {
    pub memory_id: Uuid,
    pub memory_type: MemoryType,
    pub score: f32,
    /// 初始表示层级（由分数阈值分配）
    pub tier: RepresentationTier,
    /// 完整内容的 token 估算
    pub full_tokens: u32,
}

impl BudgetItem {
    /// 指定层级下的 token 估算（与最终渲染口径一致）。
    pub fn tokens_at(&self, tier: RepresentationTier) -> u32 {
        match tier {
            RepresentationTier::Full => self.full_tokens,
            RepresentationTier::Truncated => self.full_tokens.min(200),
            RepresentationTier::Summary => self.full_tokens.min(80),
            RepresentationTier::DenseProxy => self.full_tokens.min(30),
        }
    }
}

/// 预算控制器。
#[derive(Debug, Clone)]
pub struct BudgetController {
    max_tokens: u32,
    policy: TypeBudgetPolicy,
}

impl Default for BudgetController {
    fn default() -> Self {
        BudgetController::new(4000)
    }
}

impl BudgetController {
    /// 使用默认类型策略创建控制器；`max_tokens = 0` 表示不限制。
    pub fn new(max_tokens: u32) -> Self {
        BudgetController {
            max_tokens,
            policy: TypeBudgetPolicy::default(),
        }
    }

    /// 覆盖类型预算策略。
    pub fn with_policy(mut self, policy: TypeBudgetPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn policy(&self) -> &TypeBudgetPolicy {
        &self.policy
    }

    /// 估算文本 token 数。
    ///
    /// - CJK 字符（含中日韩标点、全角符号）：1 token/字；
    /// - 其他非空白字符：0.25 token/字符；
    /// - 空白：0.1 token/字符。
    ///
    /// 结果向上取整，最小为 1（非空文本）。
    pub fn estimate_token_count(text: &str) -> u32 {
        if text.is_empty() {
            return 0;
        }
        let units: f64 = text
            .chars()
            .map(|ch| {
                if is_wide_char(ch) {
                    1.0
                } else if ch.is_whitespace() {
                    0.1
                } else {
                    0.25
                }
            })
            .sum();
        (units.ceil() as u32).max(1)
    }

    /// 按预算与类型策略筛选候选，返回按“优先级分组 + 组内分数降序”排列的结果。
    ///
    /// 不修改入参；被淘汰的候选不出现在返回列表中。`max_tokens = 0` 时原样返回
    /// （仅按分数降序排序）。
    pub fn allocate(&self, items: Vec<BudgetItem>) -> Vec<BudgetItem> {
        if self.max_tokens == 0 {
            let mut all = items;
            all.sort_by(score_cmp);
            return all;
        }

        let mut pool = items;
        pool.sort_by(score_cmp);

        let mut chosen: Vec<BudgetItem> = Vec::new();
        let mut deferred: Vec<BudgetItem> = Vec::new();
        let mut remaining = self.max_tokens;

        // 第一遍：按策略顺序逐类分配，每类受比例上限约束。
        // 上限是“最多占用”，候选不足时额度自动留给后续类型。
        for cap in &self.policy.caps {
            let cap_total =
                ((self.max_tokens as f64) * cap.ratio.clamp(0.0, 1.0) as f64).ceil() as u32;
            let mut cap_left = cap_total;

            let mut i = 0;
            while i < pool.len() {
                if pool[i].memory_type != cap.memory_type {
                    i += 1;
                    continue;
                }
                let mut item = pool.remove(i);
                match fit(&item, remaining, cap_left) {
                    Some(tier) => {
                        item.tier = tier;
                        let cost = item.tokens_at(tier);
                        remaining -= cost;
                        cap_left = cap_left.saturating_sub(cost);
                        chosen.push(item);
                    }
                    None => deferred.push(item),
                }
            }
        }

        // 第二遍：未列入策略的类型 + 因类型上限被推迟的候选，按分数消费剩余预算。
        deferred.extend(pool);
        deferred.sort_by(score_cmp);
        for item in deferred {
            if remaining == 0 {
                break;
            }
            let mut item = item;
            if let Some(tier) = fit(&item, remaining, remaining) {
                item.tier = tier;
                remaining -= item.tokens_at(tier);
                chosen.push(item);
            }
        }

        chosen
    }

    /// 纯总预算降级（不做类型分配）。保留给只关心总量、不区分类型的调用方。
    ///
    /// 返回最终总 token 数。
    pub fn check_and_downgrade(&self, items: &mut Vec<BudgetItem>) -> u32 {
        if self.max_tokens == 0 {
            return items.iter().map(|i| i.tokens_at(i.tier)).sum();
        }
        let mut remaining = self.max_tokens;
        let mut kept = Vec::with_capacity(items.len());
        for mut item in items.drain(..) {
            if let Some(tier) = fit(&item, remaining, remaining) {
                item.tier = tier;
                remaining -= item.tokens_at(tier);
                kept.push(item);
            }
        }
        *items = kept;
        items.iter().map(|i| i.tokens_at(i.tier)).sum()
    }
}

/// 在“总剩余”与“类型剩余”双重约束下，为候选找到能放下的最高层级。
fn fit(item: &BudgetItem, remaining_total: u32, remaining_type: u32) -> Option<RepresentationTier> {
    let limit = remaining_total.min(remaining_type);
    let mut tier = Some(item.tier);
    while let Some(current) = tier {
        if item.tokens_at(current) <= limit {
            return Some(current);
        }
        tier = current.downgrade();
    }
    None
}

fn score_cmp(a: &BudgetItem, b: &BudgetItem) -> std::cmp::Ordering {
    b.score
        .partial_cmp(&a.score)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a.memory_id.cmp(&b.memory_id))
}

/// 是否为宽字符（CJK 及全角标点），按 1 token/字估算。
fn is_wide_char(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF     // 韩文字母
        | 0x2E80..=0x2EFF   // CJK 部首补充
        | 0x3000..=0x303F   // CJK 标点
        | 0x3040..=0x30FF   // 日文假名
        | 0x3400..=0x4DBF   // CJK 扩展 A
        | 0x4E00..=0x9FFF   // CJK 统一表意
        | 0xA960..=0xA97F   // 韩文字母扩展 A
        | 0xAC00..=0xD7AF   // 韩文音节
        | 0xF900..=0xFAFF   // CJK 兼容表意
        | 0xFE30..=0xFE4F   // CJK 兼容形式
        | 0xFF00..=0xFFEF   // 全角/半角形式
        | 0x20000..=0x2FA1F // CJK 扩展 B 及以后
    )
}

/// 按 token 预算截断文本，尽量保留完整句子。
///
/// 截断发生在句子边界（。！？.!?）时直接补齐省略号；找不到边界或边界位置
/// 过早（不足预算的 60%）时按 token 上限硬截断。返回文本的估算 token 数
/// 不超过 `max_tokens`。
pub fn truncate_to_tokens(content: &str, max_tokens: u32) -> String {
    if max_tokens == 0 {
        return String::new();
    }
    if BudgetController::estimate_token_count(content) <= max_tokens {
        return content.to_string();
    }

    // 预留省略号的估算开销（"..." ≈ 0.75 token）。
    let limit = (max_tokens as f64) - 0.75;
    let mut units = 0.0f64;
    let mut chars: Vec<char> = Vec::new();
    let mut last_boundary: Option<usize> = None;
    let mut boundary_units = 0.0f64;

    for ch in content.chars() {
        let cost = char_cost(ch);
        if units + cost > limit {
            break;
        }
        units += cost;
        chars.push(ch);
        if is_sentence_end(ch) {
            last_boundary = Some(chars.len());
            boundary_units = units;
        }
    }

    let keep = match last_boundary {
        Some(idx) if boundary_units >= limit * 0.6 => idx,
        _ => chars.len(),
    };
    let mut result: String = chars.into_iter().take(keep).collect();
    result.push_str("...");

    // 浮点误差兜底：确保估算值不超预算。
    while BudgetController::estimate_token_count(&result) > max_tokens && !result.is_empty() {
        result.pop();
    }
    result
}

fn char_cost(ch: char) -> f64 {
    if is_wide_char(ch) {
        1.0
    } else if ch.is_whitespace() {
        0.1
    } else {
        0.25
    }
}

fn is_sentence_end(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(t: MemoryType, score: f32, tokens: u32) -> BudgetItem {
        BudgetItem {
            memory_id: Uuid::new_v4(),
            memory_type: t,
            score,
            tier: RepresentationTier::Full,
            full_tokens: tokens,
        }
    }

    #[test]
    fn test_default_budget() {
        assert_eq!(BudgetController::default().max_tokens(), 4000);
    }

    #[test]
    fn test_estimate_chinese_is_per_char() {
        // 中文按 1 token/字，不能沿用英文字符启发式（会被低估 2 倍以上）
        assert_eq!(BudgetController::estimate_token_count("中文记忆"), 4);
        assert_eq!(BudgetController::estimate_token_count("abcd"), 1);
        assert_eq!(BudgetController::estimate_token_count(""), 0);
    }

    #[test]
    fn test_estimate_mixed_content() {
        let mixed = "memrec 记忆系统";
        // "memrec"(1.5) + 空白(0.1) + 4 汉字(4.0) → 5.6 → 6
        assert_eq!(BudgetController::estimate_token_count(mixed), 6);
    }

    #[test]
    fn test_no_overflow_keeps_all() {
        let c = BudgetController::new(4000);
        let items = vec![item(MemoryType::Conversation, 0.9, 100)];
        let out = c.allocate(items);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tier, RepresentationTier::Full);
    }

    #[test]
    fn test_downgrade_before_drop() {
        let c = BudgetController::new(100);
        let items = vec![item(MemoryType::Conversation, 0.9, 500)];
        let out = c.allocate(items);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tier, RepresentationTier::Summary);
        assert!(out.iter().map(|i| i.tokens_at(i.tier)).sum::<u32>() <= 100);
    }

    #[test]
    fn test_drop_when_even_proxy_exceeds() {
        let c = BudgetController::new(10);
        let items = vec![item(MemoryType::Conversation, 0.9, 500)];
        assert!(c.allocate(items).is_empty());
    }

    #[test]
    fn test_preference_gets_priority_over_higher_score_conversation() {
        let c = BudgetController::new(100);
        let items = vec![
            item(MemoryType::Conversation, 0.99, 100),
            item(MemoryType::Preference, 0.10, 60),
        ];
        let out = c.allocate(items);
        // 偏好类型上限 50 token：降级到 Summary(60→80? no) 实际按 50 预算降级
        assert!(out.iter().any(|i| i.memory_type == MemoryType::Preference));
        assert!(out.iter().map(|i| i.tokens_at(i.tier)).sum::<u32>() <= 100);
        // 偏好排在结果最前（优先级分组顺序）
        assert_eq!(out[0].memory_type, MemoryType::Preference);
    }

    #[test]
    fn test_leftover_budget_goes_to_low_priority() {
        let c = BudgetController::new(1000);
        let items = vec![
            item(MemoryType::Preference, 0.9, 100),
            item(MemoryType::Conversation, 0.8, 700),
        ];
        let out = c.allocate(items);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out.iter()
                .find(|i| i.memory_type == MemoryType::Conversation)
                .unwrap()
                .tier,
            RepresentationTier::Full
        );
    }

    #[test]
    fn test_unlimited_keeps_everything() {
        let c = BudgetController::new(0);
        let items = vec![
            item(MemoryType::Conversation, 0.1, 9000),
            item(MemoryType::Preference, 0.9, 9000),
        ];
        assert_eq!(c.allocate(items).len(), 2);
    }

    #[test]
    fn test_truncate_keeps_sentence_boundary() {
        let text = "第一句话。第二句话。第三句话。第四句话。";
        let out = truncate_to_tokens(text, 12);
        assert!(out.ends_with("..."));
        assert!(out.contains('。'));
        assert!(BudgetController::estimate_token_count(&out) <= 12);
    }

    #[test]
    fn test_truncate_short_text_unchanged() {
        let text = "短句。";
        assert_eq!(truncate_to_tokens(text, 100), text);
    }
}
