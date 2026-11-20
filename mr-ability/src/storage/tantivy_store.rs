//! # Tantivy 全文检索存储
//!
//! 基于 Tantivy 实现 BM25 全文搜索。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
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

/// 证据强度映射的 IDF 饱和常数。
///
/// `evidence = 1 - exp(-matched_idf / (TAU * sqrt(查询词数)))`：
/// - 稀有词命中仍能得到高强度（避免误伤精确 token 检索）；
/// - 常见词、单 token 命中被显著压到阈值以下；
/// - 除以 sqrt(n) 让"词更多"的查询需要更多证据，抑制短查询虚高。
const EVIDENCE_IDF_TAU: f32 = 4.0;

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

/// 覆盖率计算时忽略的高频虚词/功能词。
///
/// 这些词几乎出现在任意中文文本中，若计入覆盖率，会把"仅共享虚词"的无关文档
/// 误判为高覆盖。仅影响 coverage 计算，不影响 Tantivy 建索引与 BM25 打分。
fn is_coverage_stopword(token: &str) -> bool {
    matches!(
        token,
        "的" | "了"
            | "是"
            | "在"
            | "和"
            | "与"
            | "或"
            | "把"
            | "被"
            | "让"
            | "而"
            | "及"
            | "等"
            | "之"
            | "者"
            | "么"
            | "呢"
            | "吗"
            | "啊"
            | "吧"
            | "就"
            | "也"
            | "都"
            | "很"
            | "更"
            | "其"
            | "这"
            | "那"
            | "我"
            | "你"
            | "他"
            | "她"
            | "它"
            | "我们"
            | "你们"
            | "他们"
            | "这个"
            | "那个"
            | "一个"
            | "什么"
            | "怎么"
            | "如何"
            | "进行"
            | "可以"
    )
}

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
            // 删除失败必须报错：旧实现用 `let _ =` 吞掉错误后继续 open_or_create，
            // 随后又把 marker 写成新版本，于是旧索引被永久判定为「已是最新」，
            // 不再重建也不报错（历史上「preview 恒空」正是这类残留）。
            std::fs::remove_dir_all(path)
                .with_context(|| format!("重建全文索引失败：无法删除 {:?}", path))?;
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

        // 写入分词器版本标记，供下次启动比对。
        //
        // marker 是「索引已按当前分词器重建」的唯一凭据，写失败必须报错：
        // 静默放过会让下次启动误判为已是最新。
        std::fs::write(&marker, TOKENIZER_VERSION)
            .with_context(|| format!("写入分词器版本标记失败：{:?}", marker))?;

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
            let mut seen = HashSet::new();
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

    /// 使用索引同款分词器切分查询，返回用于覆盖率计算的查询词集合。
    ///
    /// 过滤高频虚词，避免 `的/了/是` 等让"无关文档"也获得虚高覆盖率。
    fn query_terms(&self, query: &str) -> HashSet<String> {
        let mut terms = HashSet::new();
        let Some(mut analyzer) = self.index.tokenizers().get(TOKENIZER_NAME) else {
            return terms;
        };
        let mut stream = analyzer.token_stream(query);
        while let Some(tok) = stream.next() {
            if tok.text.is_empty() || is_coverage_stopword(&tok.text) {
                continue;
            }
            terms.insert(tok.text.clone());
        }
        terms
    }

    /// 计算文档对查询词的覆盖率（0-1）：命中的查询词数 / 查询词总数。
    ///
    /// 覆盖率用于区分"语义命中"与"仅个别字词碰撞"：单路 BM25 命中若覆盖率过低，
    /// 交由融合层丢弃，不再凭单次词项匹配获得高相关度。
    ///
    /// 同时返回**证据强度**（0-1）：以命中查询词的 IDF 之和衡量证据质量，并按
    /// 查询词数的平方根阻尼。原因：覆盖率只反映"命中比例"，单 token 查询命中
    /// 一次即等于 1.0，会把"只是出现该词"误判为确定相关（实测 `route`、
    /// `infrastructure` 等单词查询曾得到满分）。IDF 质量让稀有词命中仍然有力，
    /// 常见词命中则显著降权。
    fn coverage_and_evidence(
        &self,
        query_terms: &HashSet<String>,
        idf: &HashMap<String, f32>,
        text: &str,
    ) -> (f32, f32) {
        if query_terms.is_empty() {
            return (0.0, 0.0);
        }
        let Some(mut analyzer) = self.index.tokenizers().get(TOKENIZER_NAME) else {
            return (0.0, 0.0);
        };
        let mut stream = analyzer.token_stream(text);
        let mut seen = HashSet::new();
        let mut matched = 0usize;
        let mut matched_idf = 0.0f32;
        while let Some(tok) = stream.next() {
            if tok.text.is_empty() || !seen.insert(tok.text.clone()) {
                continue;
            }
            if query_terms.contains(&tok.text) {
                matched += 1;
                matched_idf += idf.get(&tok.text).copied().unwrap_or(0.0);
            }
        }
        let n = query_terms.len() as f32;
        let coverage = matched as f32 / n;
        // 证据 = 覆盖比例 × IDF 质量饱和度：既要求「命中足够多的查询词」，
        // 又要求「命中的词足够有区分度」，两者缺一不可。
        let idf_saturation = 1.0 - (-matched_idf / (EVIDENCE_IDF_TAU * n.sqrt())).exp();
        let evidence = coverage * idf_saturation;
        (coverage, evidence)
    }

    /// 计算单个查询词的 IDF（正文与标签两路的文档频率之和）。
    ///
    /// 仅用于证据强度计算，不参与 BM25 排序；近似处理跨字段重复计数，
    /// 只要求单调性（越稀有越大）成立。
    fn term_idf(&self, searcher: &tantivy::Searcher, term: &str) -> f32 {
        let total = searcher.num_docs() as f32;
        let mut df = 0u64;
        for field in [self.schema.content, self.schema.tags] {
            df += searcher
                .doc_freq(&tantivy::Term::from_field_text(field, term))
                .unwrap_or(0);
        }
        let df = df as f32;
        (1.0 + (total - df + 0.5) / (df + 0.5)).ln().max(0.0)
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

        // 覆盖率基于查询词集合与文档内容计算，供融合层判断"真实语义/词项命中"
        // 还是"个别字词碰撞"。查询词只切分一次；IDF 只算一次供全量文档复用。
        let query_terms = self.query_terms(query);
        let idf: HashMap<String, f32> = query_terms
            .iter()
            .map(|t| (t.clone(), self.term_idf(&searcher, t)))
            .collect();

        let mut hits = Vec::new();

        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .context("Failed to retrieve document")?;

            let id_str = doc
                .get_first(self.schema.id)
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // 索引中残留非 UUID 文档（脏数据）时直接跳过：旧实现退化为
            // `Uuid::nil()` 并作为正常命中返回，所有脏文档会坍缩成同一个 nil id
            // 互相覆盖，最终以上层查不到的 id 出现在结果里。
            let id = match Uuid::parse_str(id_str) {
                Ok(id) => id,
                Err(_) => {
                    tracing::warn!(
                        target: "fts",
                        "search: 文档 id 非合法 UUID，已跳过: {:?}",
                        id_str
                    );
                    continue;
                }
            };

            // T4: global_only 仅收全局文档（project_id 字段缺失 == None；
            // 历史 nil uuid 文档等价处理）。
            if filter.global_only {
                let is_global_doc = doc
                    .get_first(self.schema.project_id)
                    .and_then(|v| v.as_str())
                    .is_none_or(|s| {
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

            // 用正文 + 标签计算查询词覆盖率；正文过长时截取前 4000 字符，
            // 在保证代表性的同时约束单次检索的分词开销。
            let content_text = doc
                .get_first(self.schema.content)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tags_text = doc
                .get_first(self.schema.tags)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut coverage_text = String::with_capacity(content_text.len() + tags_text.len() + 1);
            coverage_text.push_str(&content_text.chars().take(4000).collect::<String>());
            coverage_text.push(' ');
            coverage_text.push_str(tags_text);
            let (coverage, evidence) =
                self.coverage_and_evidence(&query_terms, &idf, &coverage_text);

            hits.push(SearchHit {
                memory_id: id,
                // `score` 使用证据强度而非原始 BM25：BM25 量纲随查询漂移，
                // 融合层需要一个 0-1 的绝对量纲。
                score: evidence,
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
                    content_preview: content_text.chars().take(200).collect(),
                    importance: doc
                        .get_first(self.schema.importance)
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5) as f32,
                    chunk_group_id: None,
                    chunk_index: None,
                    chunk_total: None,
                },
                vector_score: None,
                fts_score: Some(score),
                coverage: Some(coverage),
                rank_score: None,
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
#[path = "tantivy_store_tests.rs"]
mod tests;
