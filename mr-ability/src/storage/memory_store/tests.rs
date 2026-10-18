//! MemoryStore 单元测试（内联 tests 抽离）。

use super::*;

    use tempfile::tempdir;

    #[tokio::test]
    async fn test_memory_save_and_get() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let memory = Memory::new("test content".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["tag1".to_string()]);

        store.save(&memory).await.unwrap();

        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "test content");
    }

    #[tokio::test]
    async fn test_memory_list() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        for i in 0..5 {
            let memory = Memory::new(format!("content {}", i), MemoryType::Conversation);
            store.save(&memory).await.unwrap();
        }

        let memories = store.list(10).await.unwrap();
        assert_eq!(memories.len(), 5);
    }

    /// 构造指定创建时间与项目的记忆（created_at 直接覆写，用于下推查询测试）。
    fn backdated_memory(content: &str, hours_ago: i64, project_id: Option<uuid::Uuid>) -> Memory {
        let mut memory = Memory::new(content.to_string(), MemoryType::Knowledge);
        memory.created_at = Utc::now() - chrono::Duration::hours(hours_ago);
        memory.project_id = project_id;
        memory
    }

    #[tokio::test]
    async fn test_created_between_index_pushdown() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let old = backdated_memory("old", 48, None);
        let mid = backdated_memory("mid", 26, Some(uuid::Uuid::new_v4()));
        // recent 显式回溯 2 小时，避免与 before 的秒级截断产生同秒歧义
        let recent = backdated_memory("recent", 2, None);

        store.save(&old).await.unwrap();
        store.save(&mid).await.unwrap();
        store.save(&recent).await.unwrap();

        // 区间 (30h 前, 20h 前]：只含 mid
        let after = (Utc::now() - chrono::Duration::hours(30)).timestamp();
        let before = (Utc::now() - chrono::Duration::hours(20)).timestamp();
        let result = store.list_created_between(after, before, 10).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, mid.id);

        // 下界之前：只含 old（recent 不落区间）
        let result = store.list_created_between(0, before, 10).await.unwrap();
        assert_eq!(result.iter().filter(|m| m.id == old.id).count(), 1);
        assert_eq!(result.iter().filter(|m| m.id == recent.id).count(), 0);

        // 上界之后：只含 recent
        let now = Utc::now().timestamp();
        let result = store.list_created_between(before, now, 10).await.unwrap();
        assert_eq!(result.iter().filter(|m| m.id == recent.id).count(), 1);

        // 软删除记录不进入查询结果
        store.delete(&mid.id).await.unwrap();
        let result = store.list_created_between(after, before, 10).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_project_index_pushdown_and_migration() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let pid = uuid::Uuid::new_v4();
        let proj = backdated_memory("proj", 48, Some(pid));
        let global_long_ago = backdated_memory("global", 40, None);
        // 历史 Dream 产出语义：Some(nil) 记全局桶
        let legacy = backdated_memory("legacy", 30, Some(uuid::Uuid::nil()));

        store.save(&proj).await.unwrap();
        store.save(&global_long_ago).await.unwrap();
        store.save(&legacy).await.unwrap();

        // 全局桶同时覆盖 None 与 Some(nil)
        let globals = store.list_global(10).await.unwrap();
        assert_eq!(globals.len(), 2);
        assert!(globals.iter().any(|m| m.id == global_long_ago.id));
        assert!(globals.iter().any(|m| m.id == legacy.id));

        // 具体项目桶精确命中
        let in_project = store.list_by_project(&pid).await.unwrap();
        assert_eq!(in_project.len(), 1);
        assert_eq!(in_project[0].id, proj.id);

        // 迁移：项目记忆改为全局后，旧项目索引项必须清理
        let mut migrated = proj.clone();
        migrated.project_id = None;
        store.save(&migrated).await.unwrap();
        assert!(store.list_by_project(&pid).await.unwrap().is_empty());
        let globals_after = store.list_global(10).await.unwrap();
        assert!(globals_after.iter().any(|m| m.id == migrated.id));
    }

    #[tokio::test]
    async fn test_reconcile_secondary_indexes_backfills() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        // 模拟历史数据：先写记忆库（老版本路径无二级索引），
        // save 现已维护索引，此处手工破坏一条的索引项来验证补写。
        let m = backdated_memory("legacy", 48, None);
        store.save(&m).await.unwrap();
        let rocksdb2 = store.rocksdb.clone();
        let cf_created = rocksdb2.cf_by_created().unwrap();
        let key = MemoryStore::created_index_key(&m.created_at, &m.id);
        rocksdb2.delete_cf(&cf_created, &key).unwrap();

        let fixed = store.reconcile_secondary_indexes().await.unwrap();
        assert_eq!(fixed, 1);

        let result = store
            .list_created_between(
                (Utc::now() - chrono::Duration::hours(50)).timestamp(),
                (Utc::now() - chrono::Duration::hours(20)).timestamp(),
                10,
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
    }

    /// T4 验证：Some(nil) 项目与 Workspace scope 在启动迁移中归一为 None / Global，
    /// 且活跃与软删记录一并处理，save 同步维护 by_project 索引。
    #[tokio::test]
    async fn test_migrate_global_semantics() {
        use mr_common::{MemoryScope, MemoryType};

        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let mut legacy_nil = Memory::new("legacy nil".to_string(), MemoryType::Knowledge);
        legacy_nil.project_id = Some(Uuid::nil());

        let mut legacy_ws = Memory::new("legacy workspace".to_string(), MemoryType::Knowledge);
        legacy_ws.project_id = Some(Uuid::nil());
        #[allow(deprecated)]
        {
            legacy_ws.scope = MemoryScope::Workspace;
        }

        let mut normal = Memory::new("normal".to_string(), MemoryType::Knowledge);
        normal.project_id = None;

        let mut project = Memory::new("in project".to_string(), MemoryType::Knowledge);
        project.project_id = Some(Uuid::new_v4());

        let mut deleted_nil = Memory::new("deleted nil".to_string(), MemoryType::Knowledge);
        deleted_nil.project_id = Some(Uuid::nil());

        store.save(&legacy_nil).await.unwrap();
        store.save(&legacy_ws).await.unwrap();
        store.save(&normal).await.unwrap();
        store.save(&project).await.unwrap();
        store.save(&deleted_nil).await.unwrap();
        store.delete(&deleted_nil.id).await.unwrap();

        // 3 条需迁移：legacy_nil、legacy_ws、deleted_nil（normal/project 原样）
        let migrated = store.migrate_global_semantics().await.unwrap();
        assert_eq!(migrated, 3);

        // 另一种载体验证：nil 记忆归一后进入全局桶（None ∪ nil 判据），with_project 索引同步
        let global_after = store.list_global(usize::MAX).await.unwrap();
        assert_eq!(global_after.len(), 3);
        assert!(global_after.iter().any(|m| m.id == legacy_nil.id));
        assert!(global_after.iter().any(|m| m.id == legacy_ws.id));

        // 具体项目记忆不受迁移影响
        let in_project = store
            .list_by_project(&project.project_id.unwrap())
            .await
            .unwrap();
        assert_eq!(in_project.len(), 1);

        // 软删的 nil 记忆同样迁移
        let restored = store.restore(&deleted_nil.id).await.unwrap().unwrap();
        assert_eq!(restored.project_id, None);

        // 幂等：再次迁移无变化
        let migrated_again = store.migrate_global_semantics().await.unwrap();
        assert_eq!(migrated_again, 0);
    }

    #[tokio::test]
    async fn test_memory_list_by_tag() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let m1 = Memory::new("content 1".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["rust".to_string()]);
        let m2 = Memory::new("content 2".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["python".to_string()]);

        store.save(&m1).await.unwrap();
        store.save(&m2).await.unwrap();

        let rust_memories = store.list_by_tag("rust", 10).await.unwrap();
        assert_eq!(rust_memories.len(), 1);
    }

    #[tokio::test]
    async fn test_memory_soft_delete() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let memory = Memory::new("test".to_string(), MemoryType::Conversation);
        store.save(&memory).await.unwrap();

        let deleted = store.delete(&memory.id).await.unwrap();
        assert!(!deleted);

        let retrieved = store.get(&memory.id).await.unwrap();
        assert!(retrieved.is_some());
        assert!(retrieved.unwrap().is_deleted);
    }

    #[tokio::test]
    async fn test_memory_restore_clears_deleted_index() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = MemoryStore::new(std::sync::Arc::new(rocksdb));

        let memory = Memory::new("test".to_string(), MemoryType::Knowledge);
        store.save(&memory).await.unwrap();

        // 软删 → count 减少
        store.delete(&memory.id).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 0);
        assert_eq!(store.count_deleted().await.unwrap(), 1);

        // 恢复 → is_deleted=false 且软删索引清理，count 恢复
        let restored = store.restore(&memory.id).await.unwrap();
        assert!(restored.is_some());
        assert!(!restored.unwrap().is_deleted);
        assert_eq!(store.count().await.unwrap(), 1);
        assert_eq!(store.count_deleted().await.unwrap(), 0);

        // 再次 restore：非删除状态返回 None
        assert!(store.restore(&memory.id).await.unwrap().is_none());
    }
