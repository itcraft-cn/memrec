//! # 场景存储实现
//!
//! [`SceneStore`] 基于 [`RocksDBStore`] 实现 [`SceneStorage`] trait，
//! 提供场景的完整生命周期管理。
//!
//! ## 存储结构
//!
//! - **scenes 列族**：UUID → JSON 序列化的 `Scene`
//! - **by_scene 列族**：`scene_id:memory_id` → UUID（场景-记忆索引）
//! - **by_heat 列族**：`heat:scene_id` → UUID（热度排序索引）
//! - **by_theme 列族**：`theme:scene_id` → UUID（主题检索索引）
//!
//! ## 热度排序
//!
//! `by_heat` 列族的键格式为 `{:010.4}:{scene_id}`，热度格式化为 10 位小数，
//! 支持按热度降序遍历。

use super::rocksdb::RocksDBStore;
use super::traits::SceneStorage;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use mr_common::Scene;
use uuid::Uuid;

/// 基于 RocksDB 的场景存储实现。
pub struct SceneStore {
    rocksdb: RocksDBStore,
}

impl SceneStore {
    /// 创建场景存储实例。
    pub fn new(rocksdb: RocksDBStore) -> Self {
        Self { rocksdb }
    }

    /// 生成场景主键（UUID 字符串的字节表示）。
    fn scene_key(id: &Uuid) -> Vec<u8> {
        id.to_string().into_bytes()
    }

    /// 生成场景-记忆索引键（`scene_id:memory_id` 格式）。
    fn scene_memory_key(scene_id: &Uuid, memory_id: &Uuid) -> Vec<u8> {
        format!("{}:{}", scene_id, memory_id).into_bytes()
    }

    /// 生成热度索引键（`heat:scene_id` 格式，热度格式化为 10 位小数）。
    fn heat_key(heat: f32, scene_id: &Uuid) -> Vec<u8> {
        format!("{:010.4}:{}", heat, scene_id).into_bytes()
    }

    /// 生成主题索引键（`theme:scene_id` 格式）。
    fn theme_key(theme: &str, scene_id: &Uuid) -> Vec<u8> {
        format!("{}:{}", theme, scene_id).into_bytes()
    }

    /// JSON 序列化场景。
    fn serialize_scene(scene: &Scene) -> Result<Vec<u8>> {
        serde_json::to_vec(scene).context("Failed to serialize scene")
    }

    /// JSON 反序列化场景。
    fn deserialize_scene(data: &[u8]) -> Result<Scene> {
        serde_json::from_slice(data).context("Failed to deserialize scene")
    }

    /// 从迭代器中收集场景，支持跳过已删除项。
    fn collect_scenes_from_iter(
        mut iter: rocksdb::DBRawIterator<'_>,
        limit: usize,
        filter_deleted: bool,
        project_id: Option<Uuid>,
    ) -> Vec<Scene> {
        let mut scenes = Vec::new();
        let mut count = 0;

        iter.seek_to_first();
        while iter.valid() && count < limit {
            if let Some(value) = iter.value() {
                if let Ok(scene) = Self::deserialize_scene(value) {
                    let matches_project = project_id
                        .map(|pid| scene.project_id == Some(pid))
                        .unwrap_or(true);
                    let not_deleted = !filter_deleted || !scene.is_deleted;

                    if matches_project && not_deleted {
                        scenes.push(scene);
                        count += 1;
                    }
                }
            }
            iter.next();
        }

        scenes
    }
}

#[async_trait]
impl SceneStorage for SceneStore {
    async fn save_scene(&self, scene: &Scene) -> Result<()> {
        let id_key = Self::scene_key(&scene.id);
        let data = Self::serialize_scene(scene)?;

        let cf_scenes = self.rocksdb.cf_scenes()?;
        self.rocksdb.put_cf(cf_scenes, &id_key, &data)?;

        for memory_id in &scene.memory_ids {
            let sm_key = Self::scene_memory_key(&scene.id, memory_id);
            let cf_by_scene = self.rocksdb.cf_by_scene()?;
            self.rocksdb.put_cf(cf_by_scene, &sm_key, &id_key)?;
        }

        let heat_key = Self::heat_key(scene.heat, &scene.id);
        let cf_by_heat = self.rocksdb.cf_by_heat()?;
        self.rocksdb.put_cf(cf_by_heat, &heat_key, &id_key)?;

        let theme_key = Self::theme_key(&scene.theme, &scene.id);
        let cf_by_theme = self.rocksdb.cf_by_theme()?;
        self.rocksdb.put_cf(cf_by_theme, &theme_key, &id_key)?;

        Ok(())
    }

    async fn get_scene(&self, id: &Uuid) -> Result<Option<Scene>> {
        let id_key = Self::scene_key(id);
        let cf_scenes = self.rocksdb.cf_scenes()?;

        match self.rocksdb.get_cf(cf_scenes, &id_key)? {
            Some(bytes) => {
                let scene = Self::deserialize_scene(&bytes)?;
                Ok(Some(scene))
            }
            None => Ok(None),
        }
    }

    async fn delete_scene(&self, id: &Uuid) -> Result<bool> {
        let scene = self.get_scene(id).await?;

        match scene {
            Some(mut sc) => {
                if sc.is_deleted {
                    let id_key = Self::scene_key(id);
                    let cf_scenes = self.rocksdb.cf_scenes()?;
                    self.rocksdb.delete_cf(cf_scenes, &id_key)?;

                    let cf_by_scene = self.rocksdb.cf_by_scene()?;
                    for memory_id in &sc.memory_ids {
                        let sm_key = Self::scene_memory_key(id, memory_id);
                        self.rocksdb.delete_cf(cf_by_scene, &sm_key)?;
                    }

                    let heat_key = Self::heat_key(sc.heat, id);
                    let cf_by_heat = self.rocksdb.cf_by_heat()?;
                    self.rocksdb.delete_cf(cf_by_heat, &heat_key)?;

                    let theme_key = Self::theme_key(&sc.theme, id);
                    let cf_by_theme = self.rocksdb.cf_by_theme()?;
                    self.rocksdb.delete_cf(cf_by_theme, &theme_key)?;

                    Ok(true)
                } else {
                    sc.is_deleted = true;
                    sc.deleted_at = Some(Utc::now());
                    self.save_scene(&sc).await?;
                    Ok(false)
                }
            }
            None => Ok(false),
        }
    }

    async fn list_scenes(
        &self,
        limit: usize,
        sort_by_heat: bool,
        project_id: Option<Uuid>,
    ) -> Result<Vec<Scene>> {
        if sort_by_heat {
            let cf_by_heat = self.rocksdb.cf_by_heat()?;
            let mut iter = self.rocksdb.iter_cf(cf_by_heat);

            let mut scenes = Vec::new();
            let mut count = 0;

            iter.seek_to_last();
            while iter.valid() && count < limit {
                if let Some(value) = iter.value() {
                    let id_str = String::from_utf8_lossy(value);
                    if let Ok(id) = Uuid::parse_str(&id_str) {
                        if let Some(scene) = self.get_scene(&id).await? {
                            let matches_project = project_id
                                .map(|pid| scene.project_id == Some(pid))
                                .unwrap_or(true);
                            if !scene.is_deleted && matches_project {
                                scenes.push(scene);
                                count += 1;
                            }
                        }
                    }
                }
                iter.prev();
            }

            Ok(scenes)
        } else {
            let cf_scenes = self.rocksdb.cf_scenes()?;
            let iter = self.rocksdb.iter_cf(cf_scenes);
            Ok(Self::collect_scenes_from_iter(iter, limit, true, project_id))
        }
    }

    async fn add_memory_to_scene(&self, scene_id: &Uuid, memory_id: &Uuid) -> Result<()> {
        let mut scene = self
            .get_scene(scene_id)
            .await?
            .context("Scene not found")?;

        scene.add_memory(*memory_id);
        self.save_scene(&scene).await
    }

    async fn remove_memory_from_scene(&self, scene_id: &Uuid, memory_id: &Uuid) -> Result<()> {
        let mut scene = self
            .get_scene(scene_id)
            .await?
            .context("Scene not found")?;

        scene.remove_memory(memory_id);

        let sm_key = Self::scene_memory_key(scene_id, memory_id);
        let cf_by_scene = self.rocksdb.cf_by_scene()?;
        self.rocksdb.delete_cf(cf_by_scene, &sm_key)?;

        self.save_scene(&scene).await
    }

    async fn get_memory_scenes(&self, memory_id: &Uuid) -> Result<Vec<Uuid>> {
        let cf_by_scene = self.rocksdb.cf_by_scene()?;
        let mut iter = self.rocksdb.iter_cf(cf_by_scene);

        let mut scene_ids = Vec::new();
        let suffix = format!(":{}", memory_id);

        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                let key_str = String::from_utf8_lossy(key);
                if key_str.ends_with(&suffix) {
                    if let Some(value) = iter.value() {
                        let id_str = String::from_utf8_lossy(value);
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            scene_ids.push(id);
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(scene_ids)
    }

    async fn update_scene_heat(&self, scene_id: &Uuid, heat: f32) -> Result<()> {
        let mut scene = self
            .get_scene(scene_id)
            .await?
            .context("Scene not found")?;

        let old_heat_key = Self::heat_key(scene.heat, scene_id);
        let cf_by_heat = self.rocksdb.cf_by_heat()?;
        self.rocksdb.delete_cf(cf_by_heat, &old_heat_key)?;

        scene.heat = heat;
        scene.updated_at = Utc::now();

        self.save_scene(&scene).await
    }

    async fn get_scene_by_theme(&self, theme: &str) -> Result<Option<Scene>> {
        let cf_by_theme = self.rocksdb.cf_by_theme()?;
        let mut iter = self.rocksdb.iter_cf(cf_by_theme);

        let prefix = format!("{}:", theme);

        iter.seek_to_first();
        while iter.valid() {
            if let Some(key) = iter.key() {
                let key_str = String::from_utf8_lossy(key);
                if key_str.starts_with(&prefix) {
                    if let Some(value) = iter.value() {
                        let id_str = String::from_utf8_lossy(value);
                        if let Ok(id) = Uuid::parse_str(&id_str) {
                            return self.get_scene(&id).await;
                        }
                    }
                }
            }
            iter.next();
        }

        Ok(None)
    }

    async fn count_scenes(&self) -> Result<usize> {
        let scenes = self.list_scenes(10000, false, None).await?;
        Ok(scenes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_scene_save_and_get() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        let scene = Scene::new("Test Scene".to_string());

        store.save_scene(&scene).await.unwrap();

        let retrieved = store.get_scene(&scene.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().theme, "Test Scene");
    }

    #[tokio::test]
    async fn test_scene_list() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        for i in 0..5 {
            let mut scene = Scene::new(format!("Scene {}", i));
            scene.heat = i as f32;
            store.save_scene(&scene).await.unwrap();
        }

        let scenes = store.list_scenes(10, false, None).await.unwrap();
        assert_eq!(scenes.len(), 5);
    }

    #[tokio::test]
    async fn test_scene_list_by_heat() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        let mut scene1 = Scene::new("Low Heat".to_string());
        scene1.heat = 1.0;
        store.save_scene(&scene1).await.unwrap();

        let mut scene2 = Scene::new("High Heat".to_string());
        scene2.heat = 10.0;
        store.save_scene(&scene2).await.unwrap();

        let scenes = store.list_scenes(10, true, None).await.unwrap();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].heat, 10.0);
        assert_eq!(scenes[1].heat, 1.0);
    }

    #[tokio::test]
    async fn test_scene_add_memory() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        let scene = Scene::new("Test".to_string());
        store.save_scene(&scene).await.unwrap();

        let memory_id = Uuid::new_v4();
        store
            .add_memory_to_scene(&scene.id, &memory_id)
            .await
            .unwrap();

        let retrieved = store.get_scene(&scene.id).await.unwrap().unwrap();
        assert_eq!(retrieved.memory_ids.len(), 1);
        assert!(retrieved.memory_ids.contains(&memory_id));
    }

    #[tokio::test]
    async fn test_scene_remove_memory() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        let scene = Scene::new("Test".to_string());
        store.save_scene(&scene).await.unwrap();

        let memory_id = Uuid::new_v4();
        store
            .add_memory_to_scene(&scene.id, &memory_id)
            .await
            .unwrap();

        store
            .remove_memory_from_scene(&scene.id, &memory_id)
            .await
            .unwrap();

        let retrieved = store.get_scene(&scene.id).await.unwrap().unwrap();
        assert_eq!(retrieved.memory_ids.len(), 0);
    }

    #[tokio::test]
    async fn test_scene_soft_delete() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        let scene = Scene::new("Test".to_string());
        store.save_scene(&scene).await.unwrap();

        let deleted = store.delete_scene(&scene.id).await.unwrap();
        assert!(!deleted);

        let retrieved = store.get_scene(&scene.id).await.unwrap().unwrap();
        assert!(retrieved.is_deleted);
    }

    #[tokio::test]
    async fn test_scene_get_memory_scenes() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        let scene1 = Scene::new("Scene 1".to_string());
        let scene2 = Scene::new("Scene 2".to_string());
        store.save_scene(&scene1).await.unwrap();
        store.save_scene(&scene2).await.unwrap();

        let memory_id = Uuid::new_v4();
        store
            .add_memory_to_scene(&scene1.id, &memory_id)
            .await
            .unwrap();
        store
            .add_memory_to_scene(&scene2.id, &memory_id)
            .await
            .unwrap();

        let scenes = store.get_memory_scenes(&memory_id).await.unwrap();
        assert_eq!(scenes.len(), 2);
    }

    #[tokio::test]
    async fn test_scene_get_by_theme() {
        let dir = tempdir().unwrap();
        let rocksdb = RocksDBStore::open(dir.path()).unwrap();
        let store = SceneStore::new(rocksdb);

        let scene = Scene::new("Unique Theme".to_string());
        store.save_scene(&scene).await.unwrap();

        let retrieved = store.get_scene_by_theme("Unique Theme").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().theme, "Unique Theme");
    }
}
