//! # Tantivy 全文检索存储
//!
//! 基于 Tantivy 实现 BM25 全文搜索。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tantivy::{
    collector::{Count, TopDocs},
    directory::MmapDirectory,
    query::{BooleanQuery, Occur, Query, QueryParser, TermQuery},
    schema::{Field, IndexRecordOption, Schema, Value, FAST, STORED, STRING, TEXT},
    tokenizer::{LowerCaser, RemoveLongFilter, TextAnalyzer, TokenStream},
    Index, IndexReader, IndexWriter, TantivyDocument,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::cn_tokenizer::CnTokenizer;
use super::traits::{FtsPayload, FtsStorage, SearchFilter, SearchHit, VectorPayload};

/// Tantivy 全文检索存储。
pub struct TantivyStore {
    index: Index,
    writer: Arc<RwLock<IndexWriter>>,
    reader: IndexReader,
    schema: TantivySchema,
}

/// Tantivy 索引 Schema。
struct TantivySchema {
    id: Field,
    content: Field,
    project_id: Field,
    memory_type: Field,
    tags: Field,
    importance: Field,
}

/// 全文检索字段使用的分词器名称（jieba 中文分词）。
const TOKENIZER_NAME: &str = "cn_jieba";

/// 索引版本标记：分词器或 FTS schema 变更时递增，触发索引目录重建。
///
/// FTS 为派生数据（以记忆库为唯一真源）。分词器升级后旧 token 与新查询的切分
/// 方式不一致，或 schema 变更（如 `content` 增加 STORED）后旧文档缺字段，
/// 都会导致旧索引不可用或命中缺数据，必须重建。
///
/// v2：`content` 字段改为 STORED，检索命中回填 `content_preview`。
const TOKENIZER_VERSION: &str = "cn_jieba_v2";

/// 版本标记文件名，写入 Tantivy 索引目录内。
const VERSION_MARKER: &str = ".tokenizer_version";

impl TantivySchema {
    fn build() -> (Schema, Self) {
        let mut schema_builder = Schema::builder();

        let id = schema_builder.add_text_field("id", STRING | STORED);
        let content = schema_builder.add_text_field(
            "content",
            TEXT.set_indexing_options(
                tantivy::schema::TextFieldIndexing::default()
                    .set_tokenizer(TOKENIZER_NAME)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            // STORED：检索命中时需回填 content_preview（仅 FTS 命中的记忆
            // 其向量侧载荷缺失，此前 preview 恒为空，削弱 Agent 判读能力）。
            .set_stored(),
        );
        let project_id = schema_builder.add_text_field("project_id", STRING | STORED);
        let memory_type = schema_builder.add_text_field("memory_type", STRING | STORED);
        let tags = schema_builder.add_text_field(
            "tags",
            TEXT.set_indexing_options(
                tantivy::schema::TextFieldIndexing::default()
                    .set_tokenizer(TOKENIZER_NAME)
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            ),
        );
        let importance = schema_builder.add_f64_field("importance", FAST | STORED);

        let schema = schema_builder.build();

        (
            schema,
            Self {
                id,
                content,
                project_id,
                memory_type,
                tags,
                importance,
            },
        )
    }
}

impl TantivyStore {
    fn register_tokenizer(index: &Index) {
        // jieba 词级切分 + 长 token 过滤 + 小写归一；中文改写查询得以命中原文。
        let tokenizer = TextAnalyzer::builder(CnTokenizer::new())
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser)
            .build();
        index.tokenizers().register(TOKENIZER_NAME, tokenizer);
    }

    /// 打开或创建 Tantivy 索引。
    ///
    /// 若检测到分词器版本变化（标记文件缺失但索引已存在，或标记不一致），
    /// 会先删除旧索引目录再新建，避免旧分词结果残留导致查询不可用；随后由
    /// 上层（daemon 启动自愈）以记忆库为真源全量重建。
    pub async fn open(path: &Path) -> Result<Self> {
        let (schema, tantivy_schema) = TantivySchema::build();

        // 分词器版本校验：旧索引与当前分词方式不兼容时强制重建。
        let marker = path.join(VERSION_MARKER);
        let stale = match std::fs::read_to_string(&marker) {
            Ok(v) => v.trim() != TOKENIZER_VERSION,
            // 标记缺失：目录内已有 meta.json 说明是旧版本创建的索引。
            Err(_) => path.join("meta.json").exists(),
        };
        if stale {
            tracing::warn!("全文检索分词器版本变化，重建索引目录: {:?}", path);
            let _ = std::fs::remove_dir_all(path);
        }

        if !path.exists() {
            std::fs::create_dir_all(path)
                .with_context(|| format!("Failed to create Tantivy directory: {:?}", path))?;
        }

        let directory = MmapDirectory::open(path)
            .with_context(|| format!("Failed to open Tantivy directory: {:?}", path))?;

        let index = Index::open_or_create(directory, schema.clone())
            .context("Failed to open or create Tantivy index")?;

        Self::register_tokenizer(&index);

        // 写入分词器版本标记，供下次启动比对；失败不影响检索，仅记录以避免阻断。
        if let Err(e) = std::fs::write(&marker, TOKENIZER_VERSION) {
            tracing::warn!("写入分词器版本标记失败: {e}");
        }

        let writer = index
            .writer(50_000_000)
            .context("Failed to create Tantivy writer")?;

        let reader = index
            .reader_builder()
            .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to create Tantivy reader")?;

        Ok(Self {
            index,
            writer: Arc::new(RwLock::new(writer)),
            reader,
            schema: tantivy_schema,
        })
    }

    /// 创建测试用的内存索引。
    pub fn new_test() -> Self {
        use tantivy::directory::RamDirectory;

        let (schema, tantivy_schema) = TantivySchema::build();
        let directory = RamDirectory::create();

        let index = Index::open_or_create(directory, schema.clone()).unwrap();
        Self::register_tokenizer(&index);

        let writer = index.writer(15_000_000).unwrap();
        let reader = index.reader_builder().try_into().unwrap();

        Self {
            index,
            writer: Arc::new(RwLock::new(writer)),
            reader,
            schema: tantivy_schema,
        }
    }

    /// 手动重载读取器，用于测试场景。
    pub fn reload(&self) -> Result<()> {
        self.reader
            .reload()
            .context("Failed to reload Tantivy reader")
    }

    /// 判断指定记忆 id 是否已在全文索引中（按 id 精确检索）。
    ///
    /// 用于启动阶段全文索引自愈：增量比对活跃记忆是否落库，避免全量重建。
    pub async fn contains(&self, id: &Uuid) -> Result<bool> {
        let searcher = self.reader.searcher();
        let term = tantivy::Term::from_field_text(self.schema.id, &id.to_string());
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        let count = searcher
            .search(&query, &Count)
            .context("Failed to query Tantivy by id")?;
        Ok(count > 0)
    }

    /// 由记忆 id、正文与载荷构建 Tantivy 文档。
    fn build_doc(&self, id: &Uuid, text: &str, payload: &FtsPayload) -> TantivyDocument {
        let mut doc = TantivyDocument::new();
        doc.add_text(self.schema.id, id.to_string());
        doc.add_text(self.schema.content, text);
        if let Some(pid) = payload.project_id {
            doc.add_text(self.schema.project_id, pid.to_string());
        }
        doc.add_text(self.schema.memory_type, payload.memory_type.clone());
        doc.add_text(self.schema.tags, payload.tags.join(" "));
        doc.add_f64(self.schema.importance, payload.importance as f64);
        doc
    }

    /// 由查询文本构建检索查询。
    ///
    /// 优先走 `QueryParser`：其短语/加权解析更贴近 BM25 的期望，排序质量更佳。
    /// 当查询包含 `+ - : ( ) " [ ]` 等会被当作语法操作符的符号、解析失败时，
    /// 回退到 [`Self::build_term_or_query`]，以「任一分词命中即召回」的宽松语义
    /// 保证检索可用（此前会直接返回错误，导致整次检索失败）。
    fn build_query(&self, query: &str) -> Box<dyn Query> {
        let parser =
            QueryParser::for_index(&self.index, vec![self.schema.content, self.schema.tags]);
        match parser.parse_query(query) {
            Ok(q) => q,
            Err(_) => self.build_term_or_query(query),
        }
    }

    /// 回退查询：复用字段分词器（jieba + LowerCaser）切词，为每个词构造
    /// `content`/`tags` 两个字段的 `Should` 词项查询并合并（OR 语义）。
    ///
    /// 该路径不做任何语法解析，因此对查询中的特殊符号天然免疫；与索引侧使用
    /// 同一分词器，保证中文切分一致。词项数量上限 256，避免构造过大的布尔查询。
    fn build_term_or_query(&self, query: &str) -> Box<dyn Query> {
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        if let Some(mut analyzer) = self.index.tokenizers().get(TOKENIZER_NAME) {
            let mut stream = analyzer.token_stream(query);
            let mut seen = std::collections::HashSet::new();
            while let Some(tok) = stream.next() {
                if tok.text.is_empty() || !seen.insert(tok.text.clone()) {
                    continue;
                }
                for field in [self.schema.content, self.schema.tags] {
                    let term = tantivy::Term::from_field_text(field, &tok.text);
                    clauses.push((
                        Occur::Should,
                        Box::new(TermQuery::new(
                            term,
                            IndexRecordOption::WithFreqsAndPositions,
                        )),
                    ));
                }
                if seen.len() >= 256 {
                    break;
                }
            }
        }
        Box::new(BooleanQuery::new(clauses))
    }

    /// 批量写入文档，仅一次提交。用于启动自愈的全量重建，避免逐条提交的开销。
    pub async fn add_many(&self, docs: &[(Uuid, String, FtsPayload)]) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        let mut writer = self.writer.write().await;
        for (id, text, payload) in docs {
            writer
                .add_document(self.build_doc(id, text, payload))
                .context("Failed to add document to Tantivy")?;
        }
        writer
            .commit()
            .context("Failed to commit Tantivy changes")?;
        Ok(())
    }

    /// 清空全文索引（删除全部文档），仅一次提交。用于启动自愈的全量重建。
    pub async fn clear(&self) -> Result<()> {
        let mut writer = self.writer.write().await;
        writer
            .delete_all_documents()
            .context("Failed to clear Tantivy index")?;
        writer.commit().context("Failed to commit Tantivy clear")?;
        Ok(())
    }
}

#[async_trait]
impl FtsStorage for TantivyStore {
    async fn add(&self, id: &Uuid, text: &str, payload: FtsPayload) -> Result<()> {
        let mut writer = self.writer.write().await;
        writer
            .add_document(self.build_doc(id, text, &payload))
            .context("Failed to add document to Tantivy")?;
        writer
            .commit()
            .context("Failed to commit Tantivy changes")?;

        Ok(())
    }

    async fn remove(&self, id: &Uuid) -> Result<bool> {
        let mut writer = self.writer.write().await;
        let term = tantivy::Term::from_field_text(self.schema.id, &id.to_string());
        writer.delete_term(term);
        writer.commit().context("Failed to commit Tantivy delete")?;
        Ok(true)
    }

    async fn search(
        &self,
        query: &str,
        filter: SearchFilter,
        top_k: usize,
    ) -> Result<Vec<SearchHit>> {
        let searcher = self.reader.searcher();

        // 复用字段分词器构建词项查询，避免 QueryParser 对原始字符串中特殊符号的
        // 语法解析失败（此前会因 `+ - : ( )` 等符号使整次检索报错）。
        let tantivy_query = self.build_query(query);

        let top_docs = searcher
            .search(&tantivy_query, &TopDocs::with_limit(top_k))
            .context("Failed to execute Tantivy search")?;

        let mut hits = Vec::new();

        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .context("Failed to retrieve document")?;

            let id_str = doc
                .get_first(self.schema.id)
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let id = Uuid::parse_str(id_str).unwrap_or_default();

            // T4: global_only 仅收全局文档（project_id 字段缺失 == None；
            // 历史 nil uuid 文档等价处理）。
            if filter.global_only {
                let is_global_doc = doc
                    .get_first(self.schema.project_id)
                    .and_then(|v| v.as_str())
                    .map_or(true, |s| {
                        s.is_empty() || Uuid::parse_str(s).map(|p| p.is_nil()).unwrap_or(false)
                    });
                if !is_global_doc {
                    continue;
                }
            } else if let Some(filter_pid) = &filter.project_id {
                let doc_pid = doc
                    .get_first(self.schema.project_id)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if doc_pid != filter_pid.to_string() && !filter.include_global {
                    continue;
                }
            }

            if let Some(filter_mt) = &filter.memory_type {
                let doc_mt = doc
                    .get_first(self.schema.memory_type)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if doc_mt != filter_mt {
                    continue;
                }
            }

            hits.push(SearchHit {
                memory_id: id,
                score,
                payload: VectorPayload {
                    project_id: doc
                        .get_first(self.schema.project_id)
                        .and_then(|v| v.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok()),
                    memory_type: doc
                        .get_first(self.schema.memory_type)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    tags: doc
                        .get_first(self.schema.tags)
                        .and_then(|v| v.as_str())
                        .map(|s| s.split_whitespace().map(String::from).collect())
                        .unwrap_or_default(),
                    // 从 STORED 的正文回填预览（截断 200 字符，与向量侧载荷口径一致）。
                    content_preview: doc
                        .get_first(self.schema.content)
                        .and_then(|v| v.as_str())
                        .map(|s| s.chars().take(200).collect())
                        .unwrap_or_default(),
                    importance: doc
                        .get_first(self.schema.importance)
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5) as f32,
                    chunk_group_id: None,
                    chunk_index: None,
                    chunk_total: None,
                },
            });
        }

        Ok(hits)
    }

    async fn count(&self) -> Result<usize> {
        let searcher = self.reader.searcher();
        Ok(searcher.num_docs() as usize)
    }

    fn reload(&self) -> Result<()> {
        self.reader
            .reload()
            .context("Failed to reload Tantivy reader")
    }
}

#[cfg(test)]
mod tests {
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
}
