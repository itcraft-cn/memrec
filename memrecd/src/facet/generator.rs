//! # Facet 生成器
//!
//! [`FacetGenerator`] trait 定义从记忆中提取主题的接口。
//! 当前提供 [`MockFacetGenerator`] 用于测试，后续可接入实际 LLM。

use anyhow::Result;
use async_trait::async_trait;
use memrec_common::{Memory, MemoryFacet};

/// Facet 生成结果。
#[derive(Debug, Clone)]
pub struct FacetGeneratorResult {
    pub facets: Vec<MemoryFacet>,
}

/// Facet 生成器 trait。
///
/// 从记忆内容中提取结构化主题（Facet）。
#[async_trait]
pub trait FacetGenerator: Send + Sync {
    /// 从单条记忆生成 Facet。
    async fn generate(&self, memory: &Memory) -> Result<FacetGeneratorResult>;

    /// 批量生成 Facet。
    async fn generate_batch(&self, memories: &[Memory]) -> Result<Vec<FacetGeneratorResult>> {
        let mut results = Vec::with_capacity(memories.len());
        for memory in memories {
            results.push(self.generate(memory).await?);
        }
        Ok(results)
    }
}

/// Mock Facet 生成器，用于测试。
///
/// 使用简单的关键词提取规则，不调用实际 LLM。
pub struct MockFacetGenerator {
    max_facets: usize,
}

impl MockFacetGenerator {
    pub fn new() -> Self {
        Self { max_facets: 5 }
    }

    pub fn with_max_facets(max_facets: usize) -> Self {
        Self { max_facets }
    }

    fn extract_keywords(content: &str) -> Vec<String> {
        let stop_words = [
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "could", "should", "may", "might",
            "must", "shall", "can", "need", "dare", "ought", "used", "to", "of", "in", "for",
            "on", "with", "at", "by", "from", "as", "into", "through", "during", "before",
            "after", "above", "below", "between", "under", "again", "further", "then", "once",
        ];

        content
            .split_whitespace()
            .filter(|word| {
                let w = word.to_lowercase();
                w.len() > 2 && !stop_words.contains(&w.as_str())
            })
            .take(10)
            .map(|s| s.to_string())
            .collect()
    }

    fn create_themes(content: &str) -> Vec<(String, f32, Vec<String>)> {
        let mut themes = Vec::new();

        if content.len() > 100 {
            themes.push((
                format!("主题: {}", content.chars().take(50).collect::<String>()),
                0.85,
                Self::extract_keywords(content),
            ));
        }

        if content.contains("偏好") || content.contains("preference") {
            themes.push((
                "用户偏好".to_string(),
                0.9,
                vec!["preference".to_string()],
            ));
        }

        if content.contains("决策") || content.contains("decision") {
            themes.push((
                "关键决策".to_string(),
                0.85,
                vec!["decision".to_string()],
            ));
        }

        if content.contains("重要") || content.contains("important") {
            themes.push((
                "重要信息".to_string(),
                0.8,
                vec!["important".to_string()],
            ));
        }

        if content.contains("Rust") || content.contains("rust") {
            themes.push((
                "Rust 技术栈".to_string(),
                0.75,
                vec!["rust".to_string()],
            ));
        }

        if themes.is_empty() {
            themes.push((
                format!("通用主题: {}", content.chars().take(30).collect::<String>()),
                0.7,
                Self::extract_keywords(content),
            ));
        }

        themes
    }
}

impl Default for MockFacetGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FacetGenerator for MockFacetGenerator {
    async fn generate(&self, memory: &Memory) -> Result<FacetGeneratorResult> {
        let themes = Self::create_themes(&memory.content);
        let mut facets = Vec::new();

        for (i, (theme, confidence, keywords)) in themes.into_iter().take(self.max_facets).enumerate() {
            let facet = MemoryFacet::new_with_evidence(
                memory.id,
                theme,
                confidence,
                keywords,
                if i == 0 { vec![memory.id] } else { vec![] },
            );
            facets.push(facet);
        }

        Ok(FacetGeneratorResult { facets })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memrec_common::MemoryType;

    #[tokio::test]
    async fn test_generate_basic_facet() {
        let generator = MockFacetGenerator::new();
        let memory = Memory::new("测试记忆内容".to_string(), MemoryType::Knowledge);

        let result = generator.generate(&memory).await.unwrap();
        assert!(!result.facets.is_empty());
    }

    #[tokio::test]
    async fn test_generate_with_preference() {
        let generator = MockFacetGenerator::new();
        let memory = Memory::new("用户偏好使用 Rust 开发".to_string(), MemoryType::Preference);

        let result = generator.generate(&memory).await.unwrap();

        let has_preference_theme = result
            .facets
            .iter()
            .any(|f| f.theme.contains("偏好"));
        assert!(has_preference_theme);
    }

    #[tokio::test]
    async fn test_generate_with_rust() {
        let generator = MockFacetGenerator::new();
        let memory = Memory::new("使用 Rust 编写高性能服务".to_string(), MemoryType::Knowledge);

        let result = generator.generate(&memory).await.unwrap();

        let has_rust_theme = result
            .facets
            .iter()
            .any(|f| f.theme.contains("Rust"));
        assert!(has_rust_theme);
    }

    #[tokio::test]
    async fn test_max_facets_limit() {
        let generator = MockFacetGenerator::with_max_facets(2);
        let memory = Memory::new(
            "用户偏好使用 Rust 开发，做出了重要决策，这是关键信息".to_string(),
            MemoryType::Decision,
        );

        let result = generator.generate(&memory).await.unwrap();
        assert!(result.facets.len() <= 2);
    }

    #[tokio::test]
    async fn test_facet_has_memory_id() {
        let generator = MockFacetGenerator::new();
        let memory = Memory::new("测试内容".to_string(), MemoryType::Knowledge);

        let result = generator.generate(&memory).await.unwrap();

        for facet in &result.facets {
            assert_eq!(facet.memory_id, memory.id);
        }
    }

    #[tokio::test]
    async fn test_batch_generate() {
        let generator = MockFacetGenerator::new();
        let memories = vec![
            Memory::new("记忆1".to_string(), MemoryType::Knowledge),
            Memory::new("记忆2".to_string(), MemoryType::Decision),
        ];

        let results = generator.generate_batch(&memories).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(!results[0].facets.is_empty());
        assert!(!results[1].facets.is_empty());
    }
}