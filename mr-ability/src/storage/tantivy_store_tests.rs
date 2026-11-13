//! tantivy_store 单元测试（内联 tests 抽离）。

use super::*;
use crate::storage::traits::FtsPayload;

fn payload() -> FtsPayload {
    FtsPayload {
        project_id: None,
        memory_type: "knowledge".to_string(),
        tags: vec!["t".to_string()],
        importance: 0.5,
    }
}

#[tokio::test]
async fn test_contains_reflects_add_and_remove() {
    let store = TantivyStore::new_test();
    let id = Uuid::new_v4();
    assert!(!store.contains(&id).await.unwrap(), "未写入前不应存在");

    store.add(&id, "hello world", payload()).await.unwrap();
    store.reload().unwrap();
    assert!(store.contains(&id).await.unwrap(), "写入后应存在");

    store.remove(&id).await.unwrap();
    store.reload().unwrap();
    assert!(!store.contains(&id).await.unwrap(), "移除后不应存在");
}

#[tokio::test]
async fn test_chinese_paraphrase_query_hits_source() {
    // 回归：中文改写查询（与原文措辞不同、无整段子串重叠）应能命中原文。
    let store = TantivyStore::new_test();
    let target = Uuid::new_v4();
    let distractor = Uuid::new_v4();
    store
        .add(
            &target,
            "BPE 项目 AGENTS.md 已按 rust 模板合并，并全部移除 Java/Python 内容",
            payload(),
        )
        .await
        .unwrap();
    store
        .add(
            &distractor,
            "完全无关的记录：某缓存系统的批量淘汰策略调参过程",
            payload(),
        )
        .await
        .unwrap();
    store.reload().unwrap();

    let hits = store
        .search(
            "把仓库的 AGENTS.md 合并成纯 Rust 模板并去掉 Java 内容",
            SearchFilter::default(),
            5,
        )
        .await
        .unwrap();
    assert!(
        hits.iter().any(|h| h.memory_id == target),
        "中文改写查询应命中原文，实际: {:?}",
        hits.iter().map(|h| h.memory_id).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_query_with_special_chars_does_not_fail() {
    // 回归：查询含 `+ - : ( ) "` 等 QueryParser 语法符号时不得整次报错。
    let store = TantivyStore::new_test();
    let id = Uuid::new_v4();
    store
        .add(
            &id,
            "分布式命名锁：epoch fencing + 迁移（SRENEW_LOST 即停续）",
            payload(),
        )
        .await
        .unwrap();
    store.reload().unwrap();

    let raw = "分布式命名锁：epoch fencing + 迁移（SRENEW_LOST 即停续）";
    let result = store.search(raw, SearchFilter::default(), 5).await;
    assert!(result.is_ok(), "含特殊符号的查询不应报错: {result:?}");
    assert!(
        result.unwrap().iter().any(|h| h.memory_id == id),
        "应命中原文"
    );
}

#[tokio::test]
async fn test_search_returns_content_preview() {
    // 回归：FTS 命中须回填 content_preview。此前 content 字段未 STORED，
    // 纯全文命中（无向量侧载荷）的 preview 恒为空串，削弱判读能力。
    let store = TantivyStore::new_test();
    let id = Uuid::new_v4();
    let content = "检索结果预览回填：FTS 命中的正文预览不得为空。";
    store.add(&id, content, payload()).await.unwrap();
    store.reload().unwrap();

    let hits = store
        .search("检索结果预览回填", SearchFilter::default(), 5)
        .await
        .unwrap();
    let hit = hits
        .iter()
        .find(|h| h.memory_id == id)
        .expect("应命中目标文档");
    assert_eq!(
        hit.payload.content_preview, content,
        "FTS 命中应回填正文预览，实际: {:?}",
        hit.payload.content_preview
    );
}

#[tokio::test]
async fn test_count_tracks_documents() {
    let store = TantivyStore::new_test();
    assert_eq!(store.count().await.unwrap(), 0);
    store.add(&Uuid::new_v4(), "a", payload()).await.unwrap();
    store.add(&Uuid::new_v4(), "b", payload()).await.unwrap();
    store.reload().unwrap();
    assert_eq!(store.count().await.unwrap(), 2);
}

#[tokio::test]
async fn test_clear_and_add_many_rebuilds() {
    let store = TantivyStore::new_test();
    store.add(&Uuid::new_v4(), "old", payload()).await.unwrap();
    store.add(&Uuid::new_v4(), "old2", payload()).await.unwrap();
    store.clear().await.unwrap();
    store.reload().unwrap();
    assert_eq!(store.count().await.unwrap(), 0, "清空后应为 0");

    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    store
        .add_many(&[
            (a, "a".to_string(), payload()),
            (b, "b".to_string(), payload()),
        ])
        .await
        .unwrap();
    store.reload().unwrap();
    assert_eq!(store.count().await.unwrap(), 2, "批量写入后应为 2");
    assert!(store.contains(&a).await.unwrap());
}

#[tokio::test]
async fn test_single_token_query_evidence_not_saturated() {
    // 回归：单 token 查询曾因 coverage=1.0 直接得到满分（`route`、`infrastructure`
    // 实测 1.0）。证据强度必须严格小于 1，且随命中词增加而增大。
    let store = TantivyStore::new_test();
    for i in 0..6 {
        store
            .add(
                &Uuid::new_v4(),
                &format!("route handler number {i} 路由处理"),
                payload(),
            )
            .await
            .unwrap();
    }
    store.reload().unwrap();

    let single = store
        .search("route", SearchFilter::default(), 5)
        .await
        .unwrap();
    assert!(!single.is_empty(), "单词查询应能命中");
    let hit = &single[0];
    assert_eq!(hit.coverage, Some(1.0), "单 token 覆盖率仍是 1.0");
    assert!(
        hit.score < 1.0,
        "单 token 证据强度不应饱和到 1.0，实际 {}",
        hit.score
    );

    let multi = store
        .search("route handler 路由", SearchFilter::default(), 5)
        .await
        .unwrap();
    assert!(
        multi[0].score > hit.score,
        "多词命中证据应高于单词命中: {} vs {}",
        multi[0].score,
        hit.score
    );
}
