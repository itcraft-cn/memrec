use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub soft_delete_recovery_days: u32,
    pub hard_delete_importance: f32,
    pub hard_delete_inactive_days: u32,
    pub compression_importance: f32,
    pub max_storage_gb: usize,
    pub high_watermark: f32,
    pub low_watermark: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            soft_delete_recovery_days: 30,
            hard_delete_importance: 0.1,
            hard_delete_inactive_days: 90,
            compression_importance: 0.3,
            max_storage_gb: 10,
            high_watermark: 0.9,
            low_watermark: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportanceConfig {
    pub lambda: f32,
    pub frequency_normalize: f32,
    pub weight_recency: f32,
    pub weight_frequency: f32,
    pub weight_semantic: f32,
    pub weight_explicit: f32,
}

impl Default for ImportanceConfig {
    fn default() -> Self {
        Self {
            lambda: 0.05,
            frequency_normalize: 10.0,
            weight_recency: 0.3,
            weight_frequency: 0.2,
            weight_semantic: 0.2,
            weight_explicit: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub socket_path: String,
    pub data_dir: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            socket_path: "~/.memrec/memrecd.sock".to_string(),
            data_dir: "~/.memrec/data".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_memory_config_defaults() {
        let config = MemoryConfig::default();
        
        assert_eq!(config.soft_delete_recovery_days, 30);
        assert_eq!(config.hard_delete_importance, 0.1);
        assert_eq!(config.max_storage_gb, 10);
        assert_eq!(config.high_watermark, 0.9);
    }
    
    #[test]
    fn test_importance_config_defaults() {
        let config = ImportanceConfig::default();
        
        assert_eq!(config.lambda, 0.05);
        assert_eq!(config.weight_recency, 0.3);
    }
    
    #[test]
    fn test_config_serde() {
        let config = MemoryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: MemoryConfig = serde_json::from_str(&json).unwrap();
        
        assert_eq!(config.soft_delete_recovery_days, parsed.soft_delete_recovery_days);
    }
}