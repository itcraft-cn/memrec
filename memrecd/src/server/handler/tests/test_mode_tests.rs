//! 测试期禁写相关测试。

use super::*;
use crate::server::{ConnMode, TestMode};

    fn router_with_test_mode(flag: std::path::PathBuf) -> Router {
        router_with_dedup(0.95, 0.80).with_test_mode(TestMode::new(flag, 3600))
    }

    #[tokio::test]
    async fn test_production_write_rejected_in_test_mode() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join(".debug");
        std::fs::write(&flag, b"{}").unwrap();
        let router = router_with_test_mode(flag.clone());

        let resp = router
            .route_with_mode(add_request("should be rejected", 1), ConnMode::Production)
            .await;
        match resp.error {
            Some(e) => assert_eq!(e.code, -32010),
            None => panic!("测试模式下生产 socket 的写入应被拒绝"),
        }
        assert_eq!(router.storage.count().await.unwrap(), 0);

        // 测试 socket 不受影响
        let resp = router
            .route_with_mode(add_request("from test socket", 2), ConnMode::TestSocket)
            .await;
        assert_eq!(extract_action(&resp), "added");
        assert_eq!(router.storage.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_production_reads_allowed_in_test_mode() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join(".debug");
        std::fs::write(&flag, b"{}").unwrap();
        let router = router_with_test_mode(flag);

        let resp = router
            .route_with_mode(
                JsonRpcRequest::new(RequestAction::Stats, None, 3),
                ConnMode::Production,
            )
            .await;
        assert!(resp.error.is_none(), "读请求不应被测试模式拦截");
        assert!(matches!(resp.result, Some(ResponseResult::Stats(_))));
    }

    #[tokio::test]
    async fn test_production_write_allowed_without_flag() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join(".debug");
        let router = router_with_test_mode(flag);

        let resp = router
            .route_with_mode(add_request("normal write", 4), ConnMode::Production)
            .await;
        assert_eq!(extract_action(&resp), "added");
    }

    #[tokio::test]
    async fn test_stale_flag_does_not_block_writes() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join(".debug");
        std::fs::write(&flag, b"{}").unwrap();
        // 修改时间回拨 2 小时，超过 max_age 3600 秒
        let f = std::fs::File::options().write(true).open(&flag).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200))
            .unwrap();

        let router = router_with_test_mode(flag);
        let resp = router
            .route_with_mode(add_request("stale flag", 5), ConnMode::Production)
            .await;
        assert_eq!(extract_action(&resp), "added");
    }
