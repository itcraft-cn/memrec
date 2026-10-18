//! 记忆命令单元测试（内联 tests 抽离）。

use super::chunk::*;


    #[test]
    fn test_split_short_content() {
        let content = "short content".to_string();
        let chunks = split_content(&content, 7500);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_split_long_content() {
        let content = "a".repeat(15000);
        let chunks = split_content(&content, 7500);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_split_preserves_utf8() {
        let content = "中文测试内容".repeat(2000);
        let chunks = split_content(&content, 7500);

        for chunk in &chunks {
            assert!(chunk.is_char_boundary(chunk.len()));
        }
    }

    #[test]
    fn test_format_part_tags() {
        let tags = vec!["rust".to_string()];
        let result = format_part_tags(&tags, 1, 3);

        assert!(result.contains(&"rust".to_string()));
        assert!(result.contains(&"part:1-3".to_string()));
        assert!(result.contains(&"part:first".to_string()));
    }
