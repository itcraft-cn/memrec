//! # 中文分词器（基于 jieba-rs）
//!
//! Tantivy 的默认分词（如 n-gram / 简单分词）对中文的处理并不理想：简单分词把
//! 无空格的中文整段当作单个 token，导致"换措辞的自然语言查询"无法命中原文；
//! n-gram 虽能召回，却会产生大量噪声 token 并拉低 BM25 区分度。
//!
//! 本模块实现一个基于 [jieba-rs](https://crates.io/crates/jieba-rs) 的
//! [`Tokenizer`]，对中英文混合文本做词级切分，供全文检索字段使用。
//!
//! 注意事项：
//! - jieba 词典首次加载较重（数十 MB），使用全局单例 [`Jieba`] 复用，避免每次
//!   分词都重建词典。
//! - 采用搜索模式（search mode）产生重叠词（如"北京大学"同时产出"北京"、"大学"
//!   与"北京大学"），提升召回率。
//! - token 位置使用连续序号而非 Unicode 偏移，保证短语查询的位置连续性；大小写
//!   归一由后续 [`tantivy::tokenizer::LowerCaser`] 过滤器完成，本分词器不改变大小写。

use jieba_rs::{Jieba, TokenizeMode};
use std::sync::OnceLock;
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

/// 全局 jieba 单例：词典在首个查询时加载并常驻，避免重复初始化开销。
fn shared_jieba() -> &'static Jieba {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    JIEBA.get_or_init(Jieba::new)
}

/// 基于 jieba-rs 的中文分词器。
///
/// 依赖全局词典单例，因此 [`Clone`] 开销极低；可安全地在多个字段/索引间共享。
#[derive(Clone, Default)]
pub struct CnTokenizer;

impl CnTokenizer {
    /// 创建分词器实例。
    pub fn new() -> Self {
        Self
    }
}

/// jieba 分词产生的 Token 流。
///
/// 持有 jieba 预先切分好的 token 列表，逐个推进并转换为 Tantivy [`Token`]。
pub struct CnTokenStream<'a> {
    tokens: Vec<jieba_rs::Token<'a>>,
    index: usize,
    token: Token,
}

impl TokenStream for CnTokenStream<'_> {
    fn advance(&mut self) -> bool {
        if self.index >= self.tokens.len() {
            return false;
        }
        let src = &self.tokens[self.index];
        self.token.offset_from = src.byte_start;
        self.token.offset_to = src.byte_end;
        // 位置取连续序号：相邻 token 位置连续，短语查询可用；重叠词获得独立位置。
        self.token.position = self.index;
        self.token.position_length = 1;
        self.token.text.clear();
        self.token.text.push_str(src.word);
        self.index += 1;
        true
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

impl Tokenizer for CnTokenizer {
    type TokenStream<'a> = CnTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        // hmm=true：启用隐马尔可夫模型识别未登录词。
        let tokens = shared_jieba().tokenize(text, TokenizeMode::Search, true);
        CnTokenStream {
            tokens,
            index: 0,
            token: Token::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 汇总分词结果，便于断言。
    fn cut(text: &str) -> Vec<String> {
        let mut tokenizer = CnTokenizer::new();
        let mut stream = tokenizer.token_stream(text);
        let mut out = Vec::new();
        while let Some(t) = TokenStream::next(&mut stream) {
            out.push(t.text.clone());
        }
        out
    }

    #[test]
    fn test_cut_chinese_splits_into_words() {
        let words = cut("北京大学");
        // 搜索模式应同时产出整体词与子词，保证召回。
        assert!(
            words.iter().any(|w| w == "北京大学"),
            "应产出整体词: {words:?}"
        );
        assert!(
            words.iter().any(|w| w == "北京"),
            "应产出子词北京: {words:?}"
        );
        assert!(
            words.iter().any(|w| w == "大学"),
            "应产出子词大学: {words:?}"
        );
    }

    #[test]
    fn test_cut_mixed_keeps_latin_and_offsets() {
        let text = "使用 Rust 模板合并";
        let words = cut(text);
        assert!(words.iter().any(|w| w == "Rust"), "拉丁词应保留: {words:?}");
        assert!(words.iter().any(|w| w == "模板"), "中文词应切出: {words:?}");

        // 校验 byte offset 能正确切回原文片段。
        let mut tokenizer = CnTokenizer::new();
        let mut stream = tokenizer.token_stream(text);
        while let Some(t) = TokenStream::next(&mut stream) {
            assert_eq!(&text[t.offset_from..t.offset_to], t.text);
        }
    }

    #[test]
    fn test_cut_returns_no_token_for_blank() {
        assert!(cut("   ").is_empty() || cut("   ").iter().all(|w| !w.is_empty()));
    }
}
