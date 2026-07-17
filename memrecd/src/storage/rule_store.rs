//! # 规则存储实现
//!
//! [`RuleStore`] 基于 [`RocksDBStore`] 实现 [`RuleStorage`] trait，
//! 提供 RetrievalRule 的完整生命周期管理。
//!
//! ## 存储结构
//!
//! - **rules 列族**：UUID → JSON 序列化的 `RetrievalRule`
//! - **rule_stats 列族**：UUID → JSON 序列化的 `RuleStats`

use super::rocksdb::RocksDBStore;
use super::traits::RuleStorage;
use anyhow::{Context, Result};
use async_trait::async_trait;
use memrec_common::{RetrievalRule, RuleStats};
use uuid::Uuid;

pub struct RuleStore {
    rocksdb: RocksDBStore,
}

impl RuleStore {
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }

    fn rule_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }

    fn serialize_rule(rule: &RetrievalRule) -> Result<Vec<u8>> {
        serde_json::to_vec(rule).context("Failed to serialize rule")
    }

    fn deserialize_rule(data: &[u8]) -> Result<RetrievalRule> {
        serde_json::from_slice(data).context("Failed to deserialize rule")
    }

    fn serialize_stats(stats: &RuleStats) -> Result<Vec<u8>> {
        serde_json::to_vec(stats).context("Failed to serialize rule stats")
    }

    fn deserialize_stats(data: &[u8]) -> Result<RuleStats> {
        serde_json::from_slice(data).context("Failed to deserialize rule stats")
    }
}

#[async_trait]
impl RuleStorage for RuleStore {
    async fn save(&self, rule: &RetrievalRule) -> Result<()> {
        let key = Self::rule_key(&rule.id);
        let value = Self::serialize_rule(rule)?;

        let cf = self.rocksdb.cf_rules()?;
        self.rocksdb.put_cf(cf, &key, &value)?;

        let stats = RuleStats::new(rule.id);
        let stats_value = Self::serialize_stats(&stats)?;
        let cf_stats = self.rocksdb.cf_rule_stats()?;
        self.rocksdb.put_cf(cf_stats, &key, &stats_value)?;

        Ok(())
    }

    async fn get(&self, id: &Uuid) -> Result<Option<RetrievalRule>> {
        let key = Self::rule_key(id);
        let cf = self.rocksdb.cf_rules()?;

        match self.rocksdb.get_cf(cf, &key)? {
            Some(bytes) => {
                let rule = Self::deserialize_rule(&bytes)?;
                Ok(Some(rule))
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, id: &Uuid) -> Result<bool> {
        let key = Self::rule_key(id);

        let cf = self.rocksdb.cf_rules()?;
        let exists = self.rocksdb.get_cf(cf, &key)?.is_some();

        if exists {
            self.rocksdb.delete_cf(cf, &key)?;

            let cf_stats = self.rocksdb.cf_rule_stats()?;
            self.rocksdb.delete_cf(cf_stats, &key)?;
        }

        Ok(exists)
    }

    async fn list(&self, enabled_only: bool) -> Result<Vec<RetrievalRule>> {
        let cf = self.rocksdb.cf_rules()?;
        let mut iter = self.rocksdb.iter_cf(cf);

        let mut rules = Vec::new();
        iter.seek_to_first();

        while iter.valid() {
            if let Some(value) = iter.value() {
                if let Ok(rule) = Self::deserialize_rule(value) {
                    if !enabled_only || rule.enabled {
                        rules.push(rule);
                    }
                }
            }
            iter.next();
        }

        rules.sort_by_key(|b| std::cmp::Reverse(b.priority));
        Ok(rules)
    }

    async fn list_by_priority(&self) -> Result<Vec<RetrievalRule>> {
        self.list(true).await
    }

    async fn update_stats(&self, stats: &RuleStats) -> Result<()> {
        let key = Self::rule_key(&stats.rule_id);
        let value = Self::serialize_stats(stats)?;

        let cf = self.rocksdb.cf_rule_stats()?;
        self.rocksdb.put_cf(cf, &key, &value)?;

        Ok(())
    }

    async fn get_stats(&self, rule_id: &Uuid) -> Result<Option<RuleStats>> {
        let key = Self::rule_key(rule_id);
        let cf = self.rocksdb.cf_rule_stats()?;

        match self.rocksdb.get_cf(cf, &key)? {
            Some(bytes) => {
                let stats = Self::deserialize_stats(&bytes)?;
                Ok(Some(stats))
            }
            None => Ok(None),
        }
    }

    async fn record_hit(&self, rule_id: &Uuid) -> Result<()> {
        let mut stats = self
            .get_stats(rule_id)
            .await?
            .unwrap_or_else(|| RuleStats::new(*rule_id));

        stats.record_hit();
        self.update_stats(&stats).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memrec_common::{BoostType, RuleAction, RuleCondition};
    use tempfile::tempdir;

    async fn create_test_store() -> RuleStore {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        RuleStore::new(rocksdb)
    }

    #[tokio::test]
    async fn test_save_and_get() {
        let store = create_test_store().await;

        let rule = RetrievalRule::new(
            "test rule".to_string(),
            "description".to_string(),
            RuleCondition::new("pattern".to_string()),
            RuleAction::boost(1.5),
        );

        store.save(&rule).await.unwrap();

        let retrieved = store.get(&rule.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test rule");
    }

    #[tokio::test]
    async fn test_delete() {
        let store = create_test_store().await;

        let rule = RetrievalRule::new(
            "test".to_string(),
            "desc".to_string(),
            RuleCondition::default(),
            RuleAction::default(),
        );

        store.save(&rule).await.unwrap();

        let deleted = store.delete(&rule.id).await.unwrap();
        assert!(deleted);

        let retrieved = store.get(&rule.id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_list_enabled_only() {
        let store = create_test_store().await;

        let mut rule1 = RetrievalRule::new(
            "enabled".to_string(),
            "desc".to_string(),
            RuleCondition::default(),
            RuleAction::default(),
        );
        rule1.enabled = true;

        let mut rule2 = RetrievalRule::new(
            "disabled".to_string(),
            "desc".to_string(),
            RuleCondition::default(),
            RuleAction::default(),
        );
        rule2.enabled = false;

        store.save(&rule1).await.unwrap();
        store.save(&rule2).await.unwrap();

        let enabled = store.list(true).await.unwrap();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "enabled");

        let all = store.list(false).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_record_hit() {
        let store = create_test_store().await;

        let rule = RetrievalRule::new(
            "test".to_string(),
            "desc".to_string(),
            RuleCondition::default(),
            RuleAction::default(),
        );

        store.save(&rule).await.unwrap();

        store.record_hit(&rule.id).await.unwrap();

        let stats = store.get_stats(&rule.id).await.unwrap().unwrap();
        assert_eq!(stats.hit_count, 1);
    }
}
