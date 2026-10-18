//! Dream 处理的纯函数辅助：相似度、摘要清洗、线索提取、门控映射、类型解析、产物构建。

use super::*;

/// 计算两个等长向量的余弦相似度；维度不一致或存在零向量时返回 0。
///
/// 用于 Dream 上游去重与语义边候选筛选的近似度量。调用方需自行保证向量
/// 已归一化或对结果量级不敏感；本函数不修改入参。
pub(super) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// 清洗 LLM 摘要输出，剥离常见套话与格式噪音。
///
/// 处理项：Markdown 代码块包裹、常见开场白前缀（循环剥离兼容嵌套）、
/// 连续空行压缩、首尾空白裁剪。不处理结尾客套（误伤风险高，交给 prompt 约束）。
pub(super) fn clean_summary(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    // 剥离 Markdown 代码块包裹（```markdown ... ``` / ``` ... ```）
    if s.starts_with("```") {
        if let Some(end) = s.rfind("```") {
            if end > 3 {
                // 去掉代码块起始标记后的语言标记行（markdown/json 等）
                let inner = &s[3..end];
                let body = match inner.find('\n') {
                    Some(nl) => &inner[nl + 1..],
                    None => inner,
                };
                s = body.trim().to_string();
            }
        }
    }

    // 循环剥离常见套话前缀（兼容 \"好的，以下是……\" 等多层嵌套）
    const PREFIXES: &[&str] = &[
        "以下是",
        "以下为",
        "以下是我",
        "以下是对",
        "好的，",
        "好的:",
        "好的：",
        "好的。",
        "总结如下",
        "摘要如下",
        "整合结果",
        "整合摘要",
        "整合后的摘要",
        "整合内容",
        "基于以上",
        "根据以上",
        "经过整合",
        "整合完成",
        "已整合",
        "这里",
        "结果如下",
        "内容如下",
    ];
    loop {
        let before = s.clone();
        for prefix in PREFIXES {
            if let Some(rest) = s.strip_prefix(prefix) {
                let rest = rest.trim_start_matches(&[':', '：', '-', '—', '\n'][..]);
                s = rest.trim_start().to_string();
                break;
            }
        }
        if s == before {
            break;
        }
    }

    // 压缩连续空行为单个空行
    let mut compact = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        compact.push_str(line);
        compact.push('\n');
        prev_blank = blank;
    }
    compact.trim_end().to_string()
}

/// 从 Dream 摘要中提取关键数据线索，用于质量评估与追溯。
///
/// 提取项：
/// - 数字数据：如 "75万条"、"4.2KB"、"134MB"、"108MB" 等
/// - 文件路径：如 "xnet_handler.rs:136"、"scenes.toml" 等
/// - 来源线索：如 "日志计数显示"、"通过gdb分析"、"代码review发现" 等
/// - 内存相关：如 "Eden"、"RSS"、"堆外"、"mmap" 等术语
/// - 行号：如 ":136"、":42" 等
pub fn extract_summary_clues(summary: &str) -> SummaryClues {
    let mut clues = SummaryClues::default();

    for line in summary.lines() {
        // 提取数字数据（含单位）
        if let Some(m) =
            lazy_regex::regex!(r"\d+([.,]\d+)?\s*(MB|GB|KB|条|次|行|天|h|小时|分钟|秒)").find(line)
        {
            clues.numbers.push(m.as_str().to_string());
        }

        // 提取文件路径（带或不带行号），完整收集（修复原有空循环死代码）
        for m in lazy_regex::regex!(r"[\w/_.-]+\.(rs|cpp|h|toml|json|log)").find_iter(line) {
            let path = m.as_str().to_string();
            if !clues.file_paths.contains(&path) {
                clues.file_paths.push(path);
            }
        }

        // 提取来源线索
        let source_clues = [
            ("日志", "log-based"),
            ("gdb", "gdb-analysis"),
            ("分析", "analysis"),
            ("计数", "count-based"),
            ("review", "code-review"),
            ("代码", "code-based"),
            ("测试", "test-based"),
            ("验证", "verified"),
        ];
        for (keyword, clue_type) in source_clues {
            if line.to_lowercase().contains(keyword) {
                clues.source_clues.insert(clue_type.to_string());
            }
        }

        // 提取内存相关术语
        let memory_terms = [
            "Eden", "RSS", "堆", "堆外", "mmap", "arena", "jemalloc", "uncommit",
        ];
        for term in memory_terms {
            if line.contains(term) || line.to_lowercase().contains(term) {
                clues.memory_terms.insert(term.to_string());
            }
        }

        // 提取行号（形如 :136），完整收集（修复原有空循环死代码）
        for cap in lazy_regex::regex!(r":\s*(\d+)").captures_iter(line) {
            if let Some(n) = cap.get(1) {
                if let Ok(num) = n.as_str().parse::<u32>() {
                    if !clues.line_numbers.contains(&num) {
                        clues.line_numbers.push(num);
                    }
                }
            }
        }
    }

    clues
}

/// 统计距 `since_unix`（Unix 秒）之后的用户新增记忆数。
///
/// 作为 Gate 新记忆门槛依据：排除 System 来源（Dream 自身产物：
/// 整合记忆/统计摘要），只统计用户侧新增（User/Inferred/External），
/// 避免 Dream 产物被计入导致门槛恒满足。
pub(super) async fn count_new_user_memories(
    storage: &Arc<dyn MemoryStorage>,
    since_unix: i64,
) -> Result<usize, DreamError> {
    let since =
        DateTime::<Utc>::from_timestamp(since_unix, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    // P0/T1: 按创建时间区间下推查询，避免全量加载后计数。
    let recent = storage
        .list_created_between(since.timestamp(), Utc::now().timestamp(), usize::MAX)
        .await
        .map_err(DreamError::from)?;
    Ok(recent
        .into_iter()
        .filter(|m| m.source != MemorySource::System)
        .count())
}

/// 把门控结果映射为 DreamRun 的 [`GateOutcome`] 与错误信息。
pub(super) fn gate_outcome(result: &DreamGateResult) -> (GateOutcome, String) {
    match result {
        DreamGateResult::Allowed => (GateOutcome::Allowed, String::new()),
        DreamGateResult::Disabled => (GateOutcome::Disabled, "Dream is disabled".to_string()),
        DreamGateResult::LlmNotConfigured => (
            GateOutcome::LlmNotConfigured,
            "Dream requires LLM, but [llm] is not configured".to_string(),
        ),
        DreamGateResult::TooSoon {
            hours_since_last,
            min_hours,
        } => (
            GateOutcome::TooSoon,
            format!("Too soon: {:.1}h < {:.1}h", hours_since_last, min_hours),
        ),
        DreamGateResult::NotEnoughNewMemories {
            new_memories,
            min_new_memories,
        } => (
            GateOutcome::NotEnoughNew,
            format!(
                "Not enough new memories: {} < {}",
                new_memories, min_new_memories
            ),
        ),
    }
}

/// 解析配置中的整合记忆类型字符串。
pub(super) fn parse_memory_type(s: &str) -> MemoryType {
    match s {
        "decision" => MemoryType::Decision,
        "knowledge" => MemoryType::Knowledge,
        "context" => MemoryType::Context,
        "preference" => MemoryType::Preference,
        _ => MemoryType::Knowledge,
    }
}

/// 构建一条 Dream 产出记忆（事实条 / 主题摘要）。
///
/// 产出统一为 System 来源、Global 范围、project_id = None（T4 全局语义统一），
/// 以便检索来源权重与可见范围稳定。
pub(super) fn build_produced_memory(
    content: String,
    memory_type: MemoryType,
    tags: Vec<String>,
    metadata: HashMap<String, String>,
    importance: f32,
) -> Memory {
    Memory {
        id: Uuid::new_v4(),
        project_id: None,
        content,
        memory_type,
        tags,
        importance,
        created_at: Utc::now(),
        last_accessed: Utc::now(),
        access_count: 1,
        source: MemorySource::System,
        scope: MemoryScope::Global,
        summary: None,
        embedding: None,
        metadata,
        is_deleted: false,
        deleted_at: None,
        chunk_group_id: None,
        chunk_index: None,
        chunk_total: None,
    }
}
