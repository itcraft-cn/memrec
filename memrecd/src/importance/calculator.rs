use chrono::{DateTime, Utc};
use memrec_common::{Memory, ImportanceConfig};
use std::collections::HashMap;

pub struct ImportanceCalculator {
    config: ImportanceConfig,
    tag_weights: HashMap<String, f32>,
}

impl ImportanceCalculator {
    pub fn new(config: ImportanceConfig) -> Self {
        Self {
            config,
            tag_weights: Self::default_tag_weights(),
        }
    }
    
    fn default_tag_weights() -> HashMap<String, f32> {
        let mut weights: HashMap<String, f32> = HashMap::new();
        weights.insert("critical".to_string(), 1.0);
        weights.insert("decision".to_string(), 0.9);
        weights.insert("key".to_string(), 0.8);
        weights.insert("important".to_string(), 0.7);
        weights.insert("config".to_string(), 0.6);
        weights.insert("reference".to_string(), 0.5);
        weights.insert("note".to_string(), 0.4);
        weights.insert("temporary".to_string(), 0.2);
        weights.insert("draft".to_string(), 0.1);
        weights
    }
    
    pub fn calculate(&self, memory: &Memory) -> f32 {
        let now = Utc::now();
        
        let recency = self.calculate_recency(memory.last_accessed, now);
        let frequency = self.calculate_frequency(memory.access_count);
        let semantic = self.calculate_semantic(&memory.tags);
        let explicit = self.calculate_explicit(&memory.metadata);
        
        let importance = self.config.weight_recency * recency
            + self.config.weight_frequency * frequency
            + self.config.weight_semantic * semantic
            + self.config.weight_explicit * explicit;
        
        importance.clamp(0.0, 1.0)
    }
    
    fn calculate_recency(&self, last_accessed: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
        let days_since_access = (now - last_accessed).num_days() as f32;
        (-self.config.lambda * days_since_access).exp()
    }
    
    fn calculate_frequency(&self, access_count: u32) -> f32 {
        ((access_count as f32 + 1.0).ln()) / self.config.frequency_normalize
    }
    
    fn calculate_semantic(&self, tags: &[String]) -> f32 {
        if tags.is_empty() {
            return 0.5;
        }
        
        tags.iter()
            .map(|tag| self.tag_weights.get(tag).copied().unwrap_or(0.5))
            .fold(0.5, |max, val| if val > max { val } else { max })
    }
    
    fn calculate_explicit(&self, metadata: &HashMap<String, String>) -> f32 {
        metadata.get("priority")
            .and_then(|p| p.parse::<f32>().ok())
            .unwrap_or(0.5)
    }
}

impl Default for ImportanceCalculator {
    fn default() -> Self {
        Self::new(ImportanceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memrec_common::MemoryType;
    
    #[test]
    fn test_calculator_new() {
        let calc = ImportanceCalculator::default();
        
        assert_eq!(calc.config.lambda, 0.05);
        assert_eq!(calc.config.weight_recency, 0.3);
    }
    
    #[test]
    fn test_calculate_full() {
        let calc = ImportanceCalculator::default();
        
        let memory = Memory::new("test".to_string(), MemoryType::Knowledge)
            .with_tags(vec!["critical".to_string()]);
        
        let importance = calc.calculate(&memory);
        
        // With critical tag (weight 1.0) and weight_semantic = 0.2
        // Expected: 0.3*recency + 0.2*frequency + 0.2*1.0 + 0.3*0.5 = ~0.6
        assert!(importance > 0.5);
    }
}