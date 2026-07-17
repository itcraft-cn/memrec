//! # Facet 生成与检索
//!
//! 提供 Facet 自动生成和主题检索功能。
//!
//! ## 模块
//!
//! - [`generator`]: Facet 生成器，从记忆提取主题
//! - [`retriever`]: Facet 检索器，支持主题级别语义搜索

pub mod generator;

pub use generator::{FacetGenerator, FacetGeneratorResult, MockFacetGenerator};