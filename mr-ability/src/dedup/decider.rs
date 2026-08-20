//! # 去重决策器实现
//!
//! 基于相似度阈值做写入决策（规则版），并为 LLM 决策演进预留接口。

use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use mr_common::SimilarMemory;

use crate::llm::{LlmClient, LlmMessage};

/// 写入决策动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupAction {
    /// 正常新增（ADD）
    Add,
    /// 视为重复，跳过写入（NOOP）
    Skip,
    /// 合并更新已有记忆（UPDATE），携带目标记忆 ID
    Update(Uuid),
}

/// 决策结果。
#[derive(Debug, Clone)]
pub struct DedupDecision {
    /// 决策动作
    pub action: DedupAction,
    /// 命中的相似候选（按相似度降序）
    pub candidates: Vec<SimilarMemory>,
}

impl DedupDecision {
    /// 正常新增决策（无候选或全部低于阈值）。
    pub fn add() -> Self {
        Self {
            action: DedupAction::Add,
            candidates: Vec::new(),
        }
    }
}

/// 去重决策器 trait。
///
/// 规则版与 LLM 版共用此接口，便于切换演进。
#[async_trait]
pub trait DedupDecider: Send + Sync {
    /// 根据新内容与相似候选做出写入决策。
    async fn decide(&self, content: &str, candidates: &[SimilarMemory]) -> DedupDecision;
}

/// 规则版决策器。
///
/// 纯阈值判断，无 LLM 依赖：
/// - `score >= duplicate_threshold`：重复，跳过
/// - `update_threshold <= score < duplicate_threshold`：合并更新（取相似度最高的）
/// - 否则：正常新增
///
/// 注意：规则版不做 DELETE（矛盾消解），避免误删；DELETE 留给 LLM 演进版。
#[derive(Debug, Clone)]
pub struct RuleDedupDecider {
    duplicate_threshold: f32,
    update_threshold: f32,
}

impl RuleDedupDecider {
    /// 创建规则决策器。
    ///
    /// # 参数约束
    ///
    /// - `duplicate_threshold` 应大于 `update_threshold`
    /// - 两个阈值都会被夹取到 [0.0, 1.0]
    pub fn new(duplicate_threshold: f32, update_threshold: f32) -> Self {
        Self {
            duplicate_threshold: duplicate_threshold.clamp(0.0, 1.0),
            update_threshold: update_threshold.clamp(0.0, 1.0),
        }
    }

    /// 从配置构造决策器。
    pub fn from_config(config: &mr_common::DedupConfig) -> Self {
        Self::new(config.duplicate_threshold, config.update_threshold)
    }
}

#[async_trait]
impl DedupDecider for RuleDedupDecider {
    async fn decide(&self, _content: &str, candidates: &[SimilarMemory]) -> DedupDecision {
        // 候选按相似度降序（调用方保证），取第一条判断即可
        if let Some(best) = candidates.first() {
            if best.score >= self.duplicate_threshold {
                return DedupDecision {
                    action: DedupAction::Skip,
                    candidates: candidates.to_vec(),
                };
            }
            if best.score >= self.update_threshold {
                return DedupDecision {
                    action: DedupAction::Update(best.memory_id),
                    candidates: candidates.to_vec(),
                };
            }
        }
        DedupDecision {
            action: DedupAction::Add,
            candidates: candidates.to_vec(),
        }
    }
}

/// LLM 演进版决策器（占位）。
///
/// 预留：后续实现 LLM 工具调用（ADD/UPDATE/DELETE/NOOP）精确判断，
/// 可识别语义矛盾并执行 DELETE。当前退化为规则行为（与 [`RuleDedupDecider`] 相同）。
#[allow(dead_code)]
pub struct LlmDedupDecider {
    llm: Arc<dyn LlmClient>,
    rule: RuleDedupDecider,
}

impl LlmDedupDecider {
    /// 创建 LLM 决策器。
    pub fn new(llm: Arc<dyn LlmClient>, duplicate_threshold: f32, update_threshold: f32) -> Self {
        Self {
            llm,
            rule: RuleDedupDecider::new(duplicate_threshold, update_threshold),
        }
    }

    /// 构造矛盾判断提示（预留：后续版本启用）。
    fn build_prompt(&self, content: &str, candidates: &[SimilarMemory]) -> String {
        let mut prompt = String::from(
            "你是记忆库维护助手。判断新记忆与已有记忆的关系，输出 JSON：\
             {\"action\": \"add|update|delete|noop\", \"target_id\": \"...\"}\n\n",
        );
        prompt.push_str("新记忆：\n");
        prompt.push_str(content);
        prompt.push('\n');
        prompt.push_str("已有记忆：\n");
        for c in candidates {
            prompt.push_str(&format!(
                "- [{:?}] {} (score={:.2})\n",
                c.memory_id, c.content, c.score
            ));
        }
        prompt
    }
}

#[async_trait]
impl DedupDecider for LlmDedupDecider {
    async fn decide(&self, content: &str, candidates: &[SimilarMemory]) -> DedupDecision {
        // TODO(v0.8): 调用 LLM 工具调用实现精确决策，当前退化为规则版
        let _ = (self
            .llm
            .chat(&[LlmMessage::user(self.build_prompt(content, candidates))])
            .await,);
        self.rule.decide(content, candidates).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: u32, score: f32) -> SimilarMemory {
        SimilarMemory {
            memory_id: Uuid::from_u128(id as u128),
            content: format!("memory {}", id),
            score,
        }
    }

    #[tokio::test]
    async fn test_duplicate_skip() {
        let decider = RuleDedupDecider::new(0.92, 0.85);
        let decision = decider.decide("new content", &[candidate(1, 0.95)]).await;
        assert_eq!(decision.action, DedupAction::Skip);
    }

    #[tokio::test]
    async fn test_update_merge() {
        let decider = RuleDedupDecider::new(0.92, 0.85);
        let decision = decider.decide("new content", &[candidate(1, 0.88)]).await;
        assert_eq!(decision.action, DedupAction::Update(Uuid::from_u128(1)));
    }

    #[tokio::test]
    async fn test_add_when_below_update_threshold() {
        let decider = RuleDedupDecider::new(0.92, 0.85);
        let decision = decider.decide("new content", &[candidate(1, 0.80)]).await;
        assert_eq!(decision.action, DedupAction::Add);
    }

    #[tokio::test]
    async fn test_add_when_no_candidates() {
        let decider = RuleDedupDecider::new(0.92, 0.85);
        let decision = decider.decide("new content", &[]).await;
        assert_eq!(decision.action, DedupAction::Add);
        assert!(decision.candidates.is_empty());
    }

    #[tokio::test]
    async fn test_candidates_kept_in_decision() {
        let decider = RuleDedupDecider::new(0.92, 0.85);
        let candidates = vec![candidate(1, 0.95), candidate(2, 0.90)];
        let decision = decider.decide("new content", &candidates).await;
        assert_eq!(decision.candidates.len(), 2);
        assert_eq!(decision.candidates[0].memory_id, Uuid::from_u128(1));
    }

    #[tokio::test]
    async fn test_threshold_clamped() {
        let decider = RuleDedupDecider::new(2.0, -1.0);
        assert_eq!(decider.duplicate_threshold, 1.0);
        assert_eq!(decider.update_threshold, 0.0);
        // update 阈值 0 意味着任何候选都会触发 update
        let decision = decider.decide("x", &[candidate(1, 0.01)]).await;
        assert_eq!(decision.action, DedupAction::Update(Uuid::from_u128(1)));
    }

    #[tokio::test]
    async fn test_llm_decider_falls_back_to_rule() {
        let llm: Arc<dyn LlmClient> = Arc::new(crate::llm::MockLlmClient::new("ignored"));
        let decider = LlmDedupDecider::new(llm, 0.92, 0.85);
        let decision = decider.decide("new content", &[candidate(1, 0.95)]).await;
        assert_eq!(decision.action, DedupAction::Skip);
    }
}
