//! # Tantivy 全文检索存储
//!
//! 基于 Tantivy 实现 BM25 全文搜索。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::QueryParser,
    schema::{Field, IndexRecordOption, Schema, Value, FAST, STORED, STRING, TEXT},
    tokenizer::{LowerCaser, NgramTokenizer, RemoveLongFilter, TextAnalyzer},
    Index, IndexReader, IndexWriter, TantivyDocument,
};
use tokio::sync::RwLock;
use uuid::Uuid;

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

const TOKENIZER_NAME: &str = "ngram_chinese";

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
            ),
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
        let tokenizer = TextAnalyzer::builder(NgramTokenizer::all_ngrams(2, 4).unwrap())
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser)
            .build();
        index.tokenizers().register(TOKENIZER_NAME, tokenizer);
    }

    /// 打开或创建 Tantivy 索引。
    pub async fn open(path: &Path) -> Result<Self> {
        let (schema, tantivy_schema) = TantivySchema::build();

        if !path.exists() {
            std::fs::create_dir_all(path)
                .with_context(|| format!("Failed to create Tantivy directory: {:?}", path))?;
        }

        let directory = MmapDirectory::open(path)
            .with_context(|| format!("Failed to open Tantivy directory: {:?}", path))?;

        let index = Index::open_or_create(directory, schema.clone())
            .context("Failed to open or create Tantivy index")?;

        Self::register_tokenizer(&index);

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
    #[cfg(test)]
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
}

#[async_trait]
impl FtsStorage for TantivyStore {
    async fn add(&self, id: &Uuid, text: &str, payload: FtsPayload) -> Result<()> {
        let mut writer = self.writer.write().await;

        let mut doc = TantivyDocument::new();
        doc.add_text(self.schema.id, id.to_string());
        doc.add_text(self.schema.content, text);

        if let Some(pid) = payload.project_id {
            doc.add_text(self.schema.project_id, pid.to_string());
        }
        doc.add_text(self.schema.memory_type, payload.memory_type);
        doc.add_text(self.schema.tags, payload.tags.join(" "));
        doc.add_f64(self.schema.importance, payload.importance as f64);

        writer
            .add_document(doc)
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

        let query_parser =
            QueryParser::for_index(&self.index, vec![self.schema.content, self.schema.tags]);

        let tantivy_query = query_parser
            .parse_query(query)
            .context("Failed to parse search query")?;

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

            if let Some(filter_pid) = &filter.project_id {
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
                    content_preview: String::new(),
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
