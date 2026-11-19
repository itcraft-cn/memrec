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

    #[test]
    fn test_parse_ttl_units() {
        let before = chrono::Utc::now();
        let at = super::parse_ttl("2h").unwrap();
        let delta = at - before;
        assert!(
            delta >= chrono::Duration::minutes(119) && delta <= chrono::Duration::minutes(121),
            "2h 应换算为约 120 分钟，实际 {delta:?}"
        );

        assert!(super::parse_ttl("7d").is_ok());
        assert!(super::parse_ttl("30m").is_ok());
        assert!(super::parse_ttl("1w").is_ok());
    }

    #[test]
    fn test_parse_ttl_rejects_invalid() {
        assert!(super::parse_ttl("7").is_err(), "缺少单位应报错");
        assert!(super::parse_ttl("0d").is_err(), "非正数应报错");
        assert!(super::parse_ttl("7y").is_err(), "未知单位应报错");
        assert!(super::parse_ttl("abc").is_err());
    }

    #[test]
    fn test_parse_expires_at() {
        let at = super::parse_expires_at("2030-01-02T03:04:05Z").unwrap();
        assert_eq!(at.timestamp(), 1_893_553_445);
        assert!(super::parse_expires_at("not-a-time").is_err());
    }
