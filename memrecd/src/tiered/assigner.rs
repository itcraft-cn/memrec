//! # 分层分配器
//!
//! 根据检索 score 分配 RepresentationTier。

use memrec_common::{RepresentationTier, TierThresholds};

#[derive(Debug, Clone)]
pub struct TierAssigner {
    thresholds: TierThresholds,
}

impl Default for TierAssigner {
    fn default() -> Self {
        TierAssigner::new(TierThresholds::default())
    }
}

impl TierAssigner {
    pub fn new(thresholds: TierThresholds) -> Self {
        TierAssigner { thresholds }
    }

    pub fn assign(&self, score: f32) -> RepresentationTier {
        self.thresholds.assign_tier(score)
    }

    pub fn assign_batch(&self, scores: &[f32]) -> Vec<RepresentationTier> {
        scores.iter().map(|s| self.assign(*s)).collect()
    }

    pub fn thresholds(&self) -> &TierThresholds {
        &self.thresholds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_assigner() {
        let assigner = TierAssigner::default();
        assert_eq!(assigner.assign(0.9), RepresentationTier::Full);
        assert_eq!(assigner.assign(0.65), RepresentationTier::Truncated);
        assert_eq!(assigner.assign(0.4), RepresentationTier::Summary);
        assert_eq!(assigner.assign(0.1), RepresentationTier::DenseProxy);
    }

    #[test]
    fn test_custom_thresholds() {
        let thresholds = TierThresholds {
            full: 0.9,
            truncated: 0.7,
            summary: 0.5,
        };
        let assigner = TierAssigner::new(thresholds);
        assert_eq!(assigner.assign(0.95), RepresentationTier::Full);
        assert_eq!(assigner.assign(0.75), RepresentationTier::Truncated);
        assert_eq!(assigner.assign(0.55), RepresentationTier::Summary);
        assert_eq!(assigner.assign(0.3), RepresentationTier::DenseProxy);
    }

    #[test]
    fn test_assign_batch() {
        let assigner = TierAssigner::default();
        let scores = vec![0.9, 0.6, 0.35, 0.1];
        let tiers = assigner.assign_batch(&scores);
        assert_eq!(tiers[0], RepresentationTier::Full);
        assert_eq!(tiers[1], RepresentationTier::Truncated);
        assert_eq!(tiers[2], RepresentationTier::Summary);
        assert_eq!(tiers[3], RepresentationTier::DenseProxy);
    }

    #[test]
    fn test_boundary_values() {
        let assigner = TierAssigner::default();
        assert_eq!(assigner.assign(0.8), RepresentationTier::Full);
        assert_eq!(assigner.assign(0.5), RepresentationTier::Truncated);
        assert_eq!(assigner.assign(0.3), RepresentationTier::Summary);
        assert_eq!(
            assigner.assign(0.2999999),
            RepresentationTier::DenseProxy
        );
    }
}
