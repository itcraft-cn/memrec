//! # 写入后的异步冲突审查（慢路径）
//!
//! 写入路径的同步规则去重只能回答“像不像”，无法判断“是否矛盾/过时”。
//! 本模块提供可选的 LLM 审查：在记忆落库后由后台任务取相似候选，交给模型判断
//! 三者关系之一：
//!
//! | 动作 | 含义 | 落地方式 |
//! |------|------|----------|
//! | `keep` | 互补/无关/不确定 | 不做任何修改 |
//! | `supersede_new` | 与旧记忆同义重复，无新增价值 | 新记忆标记 `superseded_by` 后软删 |
//! | `supersede_old` | 纠正/更新/矛盾旧记忆 | 旧记忆标记 `superseded_by`，保留新记忆 |
//!
//! ## 安全约束
//!
//! - **只做标记与软删，不改写正文**：所有动作可经 `restore` 或人工介入撤销；
//! - **先写后判**：审查发生在写入成功之后，LLM 不可用或输出非法时一律 `keep`，
//!   绝不阻塞写入、绝不因审查失败丢数据；
//! - **目标必须来自候选集**：模型返回的 `target_id` 不在候选内时降级为 `keep`。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mr_common::{SimilarMemory, ThinkLevel};
use uuid::Uuid;

use crate::llm::{LlmClient, LlmMessage};
use crate::storage::MemoryStorage;

/// 审查动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewAction {
    /// 保留新记忆（互补/无关/不确定）
    Keep,
    /// 新记忆与目标重复：标记新记忆失效并软删
    SupersedeNew(Uuid),
    /// 新记忆纠正/矛盾目标：标记目标失效
    SupersedeOld(Uuid),
}

/// 审查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDecision {
    pub action: ReviewAction,
    /// 模型给出的简短理由（用于审计日志，不落库）
    pub reason: String,
}

impl ReviewDecision {
    /// 保守决策：保留新记忆。
    pub fn keep(reason: impl Into<String>) -> Self {
        Self {
            action: ReviewAction::Keep,
            reason: reason.into(),
        }
    }
}

/// 冲突审查器 trait（便于注入 Mock 与后续替换模型/规则）。
#[async_trait]
pub trait ConflictReviewer: Send + Sync {
    async fn review(&self, content: &str, candidates: &[SimilarMemory]) -> ReviewDecision;
}

/// 基于 LLM 的冲突审查器。
pub struct LlmConflictReviewer {
    llm: Arc<dyn LlmClient>,
}

impl LlmConflictReviewer {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// 构造审查提示。
    fn build_prompt(&self, content: &str, candidates: &[SimilarMemory]) -> String {
        let mut prompt = String::from(
            "你是记忆库冲突审查器。给定一条刚写入的新记忆和若干相似旧记忆，\
             判断它们的关系并只输出一个 JSON 对象。\n\
             动作定义：\n\
             - keep：新记忆与旧记忆互补、覆盖不同信息，或无法确定；\n\
             - supersede_new：新记忆与某条旧记忆表达同一事实且没有新增价值，应删除新记忆；\n\
             - supersede_old：新记忆是对某条旧记忆的纠正、更新或矛盾，应保留新记忆并让旧记忆失效。\n\
             输出格式：{\"action\":\"keep|supersede_new|supersede_old\",\"target_id\":\"<旧记忆 UUID，keep 时留空>\",\"reason\":\"<20 字以内>\"}\n\n",
        );
        prompt.push_str("新记忆：\n");
        prompt.push_str(content);
        prompt.push_str("\n\n旧记忆：\n");
        for c in candidates {
            prompt.push_str(&format!(
                "- id={} score={:.2}\n  {}\n",
                c.memory_id, c.score, c.content
            ));
        }
        prompt
    }
}

#[async_trait]
impl ConflictReviewer for LlmConflictReviewer {
    async fn review(&self, content: &str, candidates: &[SimilarMemory]) -> ReviewDecision {
        if candidates.is_empty() {
            return ReviewDecision::keep("no candidates");
        }
        let messages = [
            LlmMessage::system("你只输出 JSON，不要输出任何解释或代码块。"),
            LlmMessage::user(self.build_prompt(content, candidates)),
        ];
        // 强制关闭思考：结构化短输出，推理内容会污染 JSON 解析。
        match self.llm.chat_with_think(&messages, ThinkLevel::Off).await {
            Ok(raw) => parse_review_response(&raw, candidates),
            Err(e) => ReviewDecision::keep(format!("llm error: {}", e)),
        }
    }
}

/// 解析模型输出为审查决策。
///
/// 容错规则：允许 ```json 围栏；JSON 非法、动作未知、`supersede_*` 缺少或
/// 携带非候选 `target_id` 时一律降级为 `keep`。
pub fn parse_review_response(raw: &str, candidates: &[SimilarMemory]) -> ReviewDecision {
    let text = raw.trim();
    let text = match (text.find("```"), text.rfind("```")) {
        (Some(start), Some(end)) if end > start => {
            let inner = &text[start + 3..end];
            inner.trim_start_matches("json").trim()
        }
        _ => text,
    };

    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return ReviewDecision::keep("invalid json");
    };
    let action = value.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let reason = value
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target = value
        .get("target_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    match action {
        "keep" => ReviewDecision {
            action: ReviewAction::Keep,
            reason,
        },
        "supersede_new" | "supersede_old" => {
            let Some(target) = target else {
                return ReviewDecision::keep("missing target_id");
            };
            if !candidates.iter().any(|c| c.memory_id == target) {
                return ReviewDecision::keep("target not in candidates");
            }
            let action = if action == "supersede_new" {
                ReviewAction::SupersedeNew(target)
            } else {
                ReviewAction::SupersedeOld(target)
            };
            ReviewDecision { action, reason }
        }
        _ => ReviewDecision::keep("unknown action"),
    }
}

/// 给记忆标记 `superseded` 并记录取代者（不覆盖正文、不改索引）。
///
/// 供写入路径的窗口外修正与后台审查共用：仅追加标签与 metadata，
/// 调用方决定是否进一步软删。
pub async fn mark_superseded(
    storage: &dyn MemoryStorage,
    target_id: &Uuid,
    superseder: &Uuid,
    reason: &str,
) -> Result<()> {
    let Some(mut old) = storage.get(target_id).await? else {
        return Ok(());
    };
    if !old.tags.iter().any(|t| t == "superseded") {
        old.tags.push("superseded".to_string());
    }
    old.metadata
        .insert("superseded_by".to_string(), superseder.to_string());
    old.metadata
        .insert("superseded_at".to_string(), chrono::Utc::now().to_rfc3339());
    if !reason.is_empty() {
        old.metadata
            .insert("superseded_reason".to_string(), reason.to_string());
    }
    storage.update(&old).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmClient;

    fn candidate(id: u32) -> SimilarMemory {
        SimilarMemory {
            memory_id: Uuid::from_u128(id as u128),
            content: format!("old {}", id),
            score: 0.8,
        }
    }

    #[test]
    fn test_parse_keep() {
        let d = parse_review_response(
            r#"{"action":"keep","target_id":"","reason":"互补"}"#,
            &[candidate(1)],
        );
        assert_eq!(d.action, ReviewAction::Keep);
    }

    #[test]
    fn test_parse_supersede_variants() {
        let id = Uuid::from_u128(1);
        let d = parse_review_response(
            &format!(
                r#"{{"action":"supersede_old","target_id":"{}","reason":"纠正"}}"#,
                id
            ),
            &[candidate(1)],
        );
        assert_eq!(d.action, ReviewAction::SupersedeOld(id));

        let d = parse_review_response(
            &format!(
                "```json\n{{\"action\":\"supersede_new\",\"target_id\":\"{}\"}}\n```",
                id
            ),
            &[candidate(1)],
        );
        assert_eq!(d.action, ReviewAction::SupersedeNew(id));
    }

    #[test]
    fn test_parse_rejects_target_outside_candidates() {
        let other = Uuid::from_u128(9);
        let d = parse_review_response(
            &format!(r#"{{"action":"supersede_old","target_id":"{}"}}"#, other),
            &[candidate(1)],
        );
        assert_eq!(d.action, ReviewAction::Keep);
    }

    #[test]
    fn test_parse_invalid_json_falls_back_to_keep() {
        let d = parse_review_response("模型拒绝输出", &[candidate(1)]);
        assert_eq!(d.action, ReviewAction::Keep);
        assert_eq!(d.reason, "invalid json");
    }

    #[tokio::test]
    async fn test_llm_reviewer_uses_response() {
        let id = Uuid::from_u128(2);
        let llm = Arc::new(MockLlmClient::new(format!(
            r#"{{"action":"supersede_old","target_id":"{}","reason":"更新"}}"#,
            id
        )));
        let reviewer = LlmConflictReviewer::new(llm);
        let d = reviewer.review("new fact", &[candidate(2)]).await;
        assert_eq!(d.action, ReviewAction::SupersedeOld(id));
    }

    #[tokio::test]
    async fn test_llm_reviewer_without_candidates_keeps() {
        let llm = Arc::new(MockLlmClient::new("{}"));
        let reviewer = LlmConflictReviewer::new(llm);
        let d = reviewer.review("new fact", &[]).await;
        assert_eq!(d.action, ReviewAction::Keep);
    }
}
