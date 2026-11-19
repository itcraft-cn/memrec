//! # Dream 规则提炼（反思的「规则出口」）
//!
//! ## 背景
//!
//! Dream 的融合阶段产出的是 summary / facts —— 仍然是“事实池”，不会收敛。
//! 而真正能改变下一次行为的，是从重复出现的决策与偏好中提炼出的**规则**
//! （“报告用项目符号”“价格不含税”“不使用旧 CRM”）。
//!
//! 本模块把用户来源的 `decision` / `preference` 记忆按项目归档，交给 LLM 归纳为
//! 至多 N 条祈使句规则，并写为**每个作用域一条**的规则集记忆：
//!
//! - 一条记忆承载整组规则，覆盖式更新（`upsert`），不追加、不堆叠；
//! - `memory_type = Preference`，复用注入预算策略中“偏好优先”的额度；
//! - `importance = 1.0`，配合排序修饰使其在相关查询中稳定靠前；
//! - 标签 `rule-set`，供注入侧按标签取用与人工排查（本模块不依赖人工复核）。
//!
//! ## 设计约束
//!
//! - 只从 `source = User` 的记录提炼：Dream 自己的产出（System）不得反过来
//!   成为规则的依据，避免自我强化导致的规则漂移；
//! - 来源不足 `min_sources` 时跳过，避免用一两条记录硬凑规则；
//! - LLM 故障视为本阶段失败，不写入半成品（保留上一版规则集）。

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;

use mr_common::{Memory, MemoryScope, MemorySource, MemoryType};
use uuid::Uuid;

use crate::llm::{LlmClient, LlmMessage};
use crate::storage::MemoryStorage;

/// 规则集记忆的标签：注入侧据此识别“常驻规则”。
pub const TAG_RULE_SET: &str = "rule-set";
/// 规则集来源标记：便于与用户手写偏好区分。
pub const TAG_DREAM_RULE: &str = "dream-rule";

/// 单个作用域送进提示词的最大来源条数（取最近的一批）。
const MAX_SOURCES_PER_GROUP: usize = 60;
/// 单轮最多提炼的作用域数：按来源条数取前 N，控制 LLM 调用成本。
const MAX_GROUPS_PER_RUN: usize = 3;

/// 单次提炼的统计。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DistillReport {
    /// 参与归纳的来源记忆数
    pub scanned: usize,
    /// 成功写入（新增或覆盖）的规则集数量，按作用域计
    pub written: usize,
    /// 被跳过的作用域数（来源不足或 LLM 输出为空）
    pub skipped: usize,
}

/// LLM 期望输出。
#[derive(Debug, Deserialize)]
struct RuleOutput {
    #[serde(default)]
    rules: Vec<String>,
}

/// 归纳提示词（固定，不含变量插值）。
const SYSTEM_PROMPT: &str = "你是记忆整合器。输入是同一项目积累的决策与偏好记录。\n\
请把它们归纳为可长期复用的行为规则：\n\
1. 每条一行，祈使句，必须能直接指导未来的具体做法；\n\
2. 只保留跨记录重复出现、或被明确要求的约束；一次性事件不要写成规则；\n\
3. 不得编造原文没有的约束，不得输出泛泛而谈的建议；\n\
4. 合并同义项，按重要性排序；\n\
5. 只输出 JSON：{\"rules\":[\"...\"]}，不要解释、不要代码块。";

/// 从决策/偏好记忆中提炼规则集。
///
/// `storage` 应为索引一致型存储，使覆盖更新后索引同步刷新；
/// `llm` 为唯一外部依赖，调用失败时整轮不写入。
pub async fn distill(
    storage: &Arc<dyn MemoryStorage>,
    llm: &Arc<dyn LlmClient>,
    max_rules: usize,
    min_sources: usize,
) -> anyhow::Result<DistillReport> {
    let max_rules = max_rules.max(1);

    // 取数上限：一次提炼只处理最近的一批，避免提示词随库增长而膨胀。
    let mut sources: Vec<Memory> = Vec::new();
    for mtype in [MemoryType::Decision, MemoryType::Preference] {
        sources.extend(storage.list_by_type(mtype, 300).await?);
    }
    sources.retain(|m| {
        !m.is_deleted
            && m.source == MemorySource::User
            && !m.is_dream_generated()
            && !m.tags.iter().any(|t| t == TAG_RULE_SET)
            && m.content.trim().chars().count() >= 8
    });
    // 新 → 旧，保证提示词稳定且优先采用较新的约束。
    sources.sort_by_key(|b| std::cmp::Reverse(b.created_at));

    let mut report = DistillReport {
        scanned: sources.len(),
        ..Default::default()
    };

    // 按项目归组；`None` 为全局作用域。
    let mut groups: BTreeMap<Option<Uuid>, Vec<&Memory>> = BTreeMap::new();
    for m in &sources {
        groups.entry(m.project_id).or_default().push(m);
    }

    // 成本控制：单轮最多处理来源最丰富的 N 个作用域。
    let mut groups: Vec<(Option<Uuid>, Vec<&Memory>)> = groups.into_iter().collect();
    groups.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));

    // 规则集一次取全，供增量判定与覆盖写复用。
    let existing_sets: Vec<Memory> = storage
        .list_by_tag(TAG_RULE_SET, 1_000)
        .await?
        .into_iter()
        .filter(|m| !m.is_deleted)
        .collect();

    for (project_id, members) in groups.into_iter().take(MAX_GROUPS_PER_RUN) {
        if members.len() < min_sources {
            report.skipped += 1;
            continue;
        }

        // 增量门槛：本作用域没有比现有规则集更新的来源时，跳过本轮。
        // 这条规则避免“没有新证据也每天重算一次”的固定 token 消耗。
        if !has_new_evidence(&existing_sets, project_id, &members) {
            report.skipped += 1;
            continue;
        }

        // 只取最近的 N 条，防止提示词随库增长线性膨胀。
        let members: Vec<&Memory> = members.into_iter().take(MAX_SOURCES_PER_GROUP).collect();

        let raw = match llm
            .chat_with_think(&build_messages(&members), mr_common::ThinkLevel::Off)
            .await
        {
            Ok(r) => r,
            // 任一作用域失败即整轮中止：避免“部分作用域更新、部分停留在旧版本”
            // 造成规则集版本不一致。
            Err(e) => return Err(anyhow::anyhow!("rule distillation LLM failed: {e}")),
        };

        let rules = match parse_rules(&raw, max_rules) {
            Some(r) if !r.is_empty() => r,
            _ => {
                report.skipped += 1;
                continue;
            }
        };

        let content = render_rules(&rules);
        let source_ids: Vec<String> = members.iter().take(50).map(|m| m.id.to_string()).collect();
        upsert_rule_set(
            &existing_sets,
            storage,
            project_id,
            content,
            members.len(),
            source_ids,
        )
        .await?;
        report.written += 1;
    }

    Ok(report)
}

/// 判断某作用域是否有比现有规则集更新的来源记忆。
///
/// 无现有规则集时视为有（需要首次生成）。时间以规则集 `metadata.updated_at`
/// 为准；缺失该字段时按“有”处理，保证旧数据也能升级。
fn has_new_evidence(
    existing_sets: &[Memory],
    project_id: Option<Uuid>,
    members: &[&Memory],
) -> bool {
    let Some(existing) = existing_sets.iter().find(|m| m.project_id == project_id) else {
        return true;
    };
    let Some(updated_at) = existing
        .metadata
        .get("updated_at")
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&chrono::Utc))
    else {
        return true;
    };
    members
        .iter()
        .any(|m| m.created_at > updated_at)
}

/// 构造消息：系统提示 + 编号来源列表。
fn build_messages(members: &[&Memory]) -> Vec<LlmMessage> {
    let mut user = String::from("来源记录（类型 | 内容）：\n");
    for (i, m) in members.iter().enumerate() {
        let body = m.content.replace(['\n', '\r'], " ");
        user.push_str(&format!("[{}] {} | {}\n", i, m.memory_type, body));
    }
    vec![LlmMessage::system(SYSTEM_PROMPT), LlmMessage::user(user)]
}

/// 解析模型输出：容忍代码块包裹与前后噪声。
fn parse_rules(raw: &str, max_rules: usize) -> Option<Vec<String>> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end < start {
        return None;
    }
    let parsed: RuleOutput = serde_json::from_str(&raw[start..=end]).ok()?;
    let mut rules: Vec<String> = parsed
        .rules
        .into_iter()
        .map(|r| r.trim().trim_start_matches(['-', '*', ' ']).to_string())
        .filter(|r| !r.is_empty())
        .collect();
    rules.truncate(max_rules);
    Some(rules)
}

/// 渲染为可注入的规则正文：每条一行，前缀 `- `。
fn render_rules(rules: &[String]) -> String {
    rules
        .iter()
        .map(|r| format!("- {}", r))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 覆盖式写入某作用域的规则集。
///
/// 同一作用域只保留一条：更新最新一条的内容与元数据，删除多余旧条目。
/// 这样规则集不会随时间线膨胀（“反思”必须收敛，否则退化为事实流水账）。
async fn upsert_rule_set(
    existing_sets: &[Memory],
    storage: &Arc<dyn MemoryStorage>,
    project_id: Option<Uuid>,
    content: String,
    source_count: usize,
    source_ids: Vec<String>,
) -> anyhow::Result<()> {
    let mut mine: Vec<Memory> = existing_sets
        .iter()
        .filter(|m| m.project_id == project_id)
        .cloned()
        .collect();
    mine.sort_by_key(|b| std::cmp::Reverse(b.created_at));

    if let Some(latest) = mine.first() {
        // 覆盖写：内容变化时索引装饰器会重建向量与全文；内容未变也会刷新
        // 元数据，保证 `updated_at`/`source_count` 反映本轮归纳。
        let mut updated = latest.clone();
        updated.content = content;
        updated.importance = 1.0;
        updated.source = MemorySource::System;
        updated.scope = scope_for(project_id);
        updated.last_accessed = chrono::Utc::now();
        updated
            .metadata
            .insert("updated_at".to_string(), chrono::Utc::now().to_rfc3339());
        updated
            .metadata
            .insert("source_count".to_string(), source_count.to_string());
        updated.metadata.insert(
            "source_ids".to_string(),
            serde_json::to_string(&source_ids).unwrap_or_default(),
        );
        let id = updated.id;
        storage.update(&updated).await?;
        let duplicates: Vec<Memory> = mine.into_iter().skip(1).collect();
        for dup in duplicates {
            let _ = storage.delete(&dup.id).await;
        }
        tracing::debug!(target: "dream", "rule set upserted (update): {}", id);
        return Ok(());
    }

    let mut memory = Memory::new(content, MemoryType::Preference)
        .with_tags(vec![TAG_RULE_SET.to_string(), TAG_DREAM_RULE.to_string()])
        .with_scope(scope_for(project_id))
        .with_source(MemorySource::System);
    memory.importance = 1.0;
    memory.project_id = project_id;
    memory
        .metadata
        .insert("updated_at".to_string(), chrono::Utc::now().to_rfc3339());
    memory
        .metadata
        .insert("source_count".to_string(), source_count.to_string());
    memory.metadata.insert(
        "source_ids".to_string(),
        serde_json::to_string(&source_ids).unwrap_or_default(),
    );
    storage.add(&memory).await?;
    tracing::debug!(target: "dream", "rule set created: {}", memory.id);
    Ok(())
}

/// 项目记忆 → `Project`，全局记忆 → `Global`。
fn scope_for(project_id: Option<Uuid>) -> MemoryScope {
    if project_id.is_some() {
        MemoryScope::Project
    } else {
        MemoryScope::Global
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rules_extracts_json() {
        let raw = "```json\n{\"rules\":[\"用项目符号\",\" 价格不含税 \",\"\"]}\n```";
        let rules = parse_rules(raw, 10).unwrap();
        assert_eq!(rules, vec!["用项目符号", "价格不含税"]);
    }

    #[test]
    fn test_parse_rules_truncates_to_max() {
        let raw = r#"{"rules":["a","b","c","d"]}"#;
        assert_eq!(parse_rules(raw, 2).unwrap().len(), 2);
    }

    #[test]
    fn test_parse_rules_rejects_garbage() {
        assert!(parse_rules("no json here", 5).is_none());
    }

    #[test]
    fn test_render_rules_is_one_per_line() {
        let text = render_rules(&["a".to_string(), "b".to_string()]);
        assert_eq!(text, "- a\n- b");
    }

    #[test]
    fn test_scope_for() {
        assert_eq!(scope_for(None), MemoryScope::Global);
        assert_eq!(scope_for(Some(Uuid::new_v4())), MemoryScope::Project);
    }

    async fn store() -> (Arc<dyn MemoryStorage>, tempfile::TempDir) {
        use crate::storage::{MemoryStore, RocksDBStore};
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(RocksDBStore::open(dir.path()).unwrap());
        (Arc::new(MemoryStore::new(db)), dir)
    }

    async fn add_sources(storage: &Arc<dyn MemoryStorage>, project: Option<Uuid>, n: usize) {
        for i in 0..n {
            let mut m = Memory::new(
                format!("决策 {i}：报告必须使用项目符号且价格不含税"),
                MemoryType::Decision,
            )
            .with_source(MemorySource::User);
            if let Some(pid) = project {
                m = m.with_project(pid);
            }
            storage.save(&m).await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_distill_creates_single_rule_set() {
        use crate::llm::MockLlmClient;

        let (storage, _dir) = store().await;
        let pid = Uuid::new_v4();
        add_sources(&storage, Some(pid), 5).await;

        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(
            r#"{"rules":["报告用项目符号","价格不含税"]}"#,
        ));
        let report = distill(&storage, &llm, 20, 5).await.unwrap();
        assert_eq!(report.scanned, 5);
        assert_eq!(report.written, 1);

        let sets = storage.list_by_tag(TAG_RULE_SET, 10).await.unwrap();
        assert_eq!(sets.len(), 1, "每个作用域只允许一条规则集");
        let set = &sets[0];
        assert_eq!(set.content, "- 报告用项目符号\n- 价格不含税");
        assert_eq!(set.memory_type, MemoryType::Preference);
        assert_eq!(set.project_id, Some(pid));
        assert_eq!(set.scope, MemoryScope::Project);
        assert_eq!(set.importance, 1.0);
    }

    #[tokio::test]
    async fn test_distill_overwrites_without_growing() {
        use crate::llm::MockLlmClient;

        let (storage, _dir) = store().await;
        add_sources(&storage, None, 5).await;

        let first: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(r#"{"rules":["规则A"]}"#));
        distill(&storage, &first, 20, 5).await.unwrap();

        // 新增一条来源（时间晚于上一版规则集），触发增量重算。
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        add_sources(&storage, None, 1).await;

        let second: Arc<dyn LlmClient> =
            Arc::new(MockLlmClient::new(r#"{"rules":["规则A","规则B"]}"#));
        distill(&storage, &second, 20, 5).await.unwrap();

        let sets = storage.list_by_tag(TAG_RULE_SET, 10).await.unwrap();
        assert_eq!(sets.len(), 1, "覆盖式更新不得随轮次增长");
        assert_eq!(sets[0].content, "- 规则A\n- 规则B");
    }

    #[tokio::test]
    async fn test_distill_skips_without_new_evidence() {
        use crate::llm::MockLlmClient;

        let (storage, _dir) = store().await;
        add_sources(&storage, None, 5).await;

        let first: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(r#"{"rules":["规则A"]}"#));
        distill(&storage, &first, 20, 5).await.unwrap();

        // 没有更新的来源：第二轮应直接跳过，不再调用 LLM。
        let second: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new("这不是 JSON"));
        let report = distill(&storage, &second, 20, 5).await.unwrap();
        assert_eq!(report.written, 0);
        assert_eq!(report.skipped, 1);

        let sets = storage.list_by_tag(TAG_RULE_SET, 10).await.unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].content, "- 规则A", "无新证据时不得重写规则集");
    }

    #[tokio::test]
    async fn test_distill_skips_when_sources_insufficient() {
        use crate::llm::MockLlmClient;

        let (storage, _dir) = store().await;
        add_sources(&storage, Some(Uuid::new_v4()), 2).await;

        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(r#"{"rules":["不应写入"]}"#));
        let report = distill(&storage, &llm, 20, 5).await.unwrap();
        assert_eq!(report.written, 0);
        assert_eq!(report.skipped, 1);
        assert!(storage
            .list_by_tag(TAG_RULE_SET, 10)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_distill_ignores_system_sources() {
        use crate::llm::MockLlmClient;

        let (storage, _dir) = store().await;
        let pid = Uuid::new_v4();
        for i in 0..6 {
            let m = Memory::new(
                format!("系统产出 {i} 不应成为规则依据"),
                MemoryType::Decision,
            )
            .with_source(MemorySource::System)
            .with_project(pid);
            storage.save(&m).await.unwrap();
        }

        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(r#"{"rules":["不应写入"]}"#));
        let report = distill(&storage, &llm, 20, 5).await.unwrap();
        assert_eq!(report.scanned, 0);
        assert_eq!(report.written, 0);
    }
}
