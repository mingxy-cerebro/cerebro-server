use crate::domain::memory::Memory;
use crate::domain::types::Tier;

use super::decay::{parse_days_ago, DecayConfig, DecayEngine};

pub struct TierConfig {
    pub working_access_threshold: u32,
    pub working_composite_threshold: f32,
    pub core_access_threshold: u32,
    pub core_composite_threshold: f32,
    pub core_importance_threshold: f32,
    pub peripheral_composite_threshold: f32,
    pub peripheral_age_days: f32,
}

impl Default for TierConfig {
    fn default() -> Self {
        Self {
            working_access_threshold: 3,
            working_composite_threshold: 0.4,
            core_access_threshold: 10,
            core_composite_threshold: 0.7,
            core_importance_threshold: 0.8,
            peripheral_composite_threshold: 0.15,
            peripheral_age_days: 60.0,
        }
    }
}

pub struct TierManager {
    config: TierConfig,
    decay: DecayEngine,
}

impl TierManager {
    pub fn new(config: TierConfig, decay: DecayEngine) -> Self {
        Self { config, decay }
    }

    pub fn with_defaults() -> Self {
        Self::new(
            TierConfig::default(),
            DecayEngine::new(DecayConfig::default()),
        )
    }

    pub fn evaluate_tier(&self, memory: &Memory) -> Tier {
        let composite = self.decay.compute_composite(memory);
        let age_days = parse_days_ago(&memory.created_at);

        match memory.tier {
            Tier::Peripheral => {
                if memory.access_count >= self.config.working_access_threshold
                    && composite >= self.config.working_composite_threshold
                {
                    Tier::Working
                } else {
                    Tier::Peripheral
                }
            }
            Tier::Working => {
                if memory.access_count >= self.config.core_access_threshold
                    && composite >= self.config.core_composite_threshold
                    && memory.importance >= self.config.core_importance_threshold
                {
                    Tier::Core
                } else if composite < self.config.peripheral_composite_threshold
                    || (age_days > self.config.peripheral_age_days
                        && memory.access_count < self.config.working_access_threshold)
                {
                    Tier::Peripheral
                } else {
                    Tier::Working
                }
            }
            Tier::Core => {
                if composite < self.config.peripheral_composite_threshold
                    && memory.access_count < self.config.working_access_threshold
                {
                    Tier::Working
                } else {
                    Tier::Core
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::Category;
    use crate::domain::types::MemoryType;
    use chrono::Utc;

    fn days_ago_str(n: i64) -> String {
        let delta = chrono::TimeDelta::try_days(n).unwrap_or_default();
        (Utc::now() - delta).to_rfc3339()
    }

    fn make_test_memory() -> Memory {
        let mut mem = Memory::new(
            "test memory",
            Category::Preferences,
            MemoryType::Insight,
            "t-test",
        );
        mem.created_at = days_ago_str(0);
        mem.updated_at = mem.created_at.clone();
        mem
    }

    #[test]
    fn test_promotion_to_working() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Peripheral;
        mem.access_count = 3;
        mem.importance = 0.5;
        mem.confidence = 0.5;
        mem.created_at = days_ago_str(1);
        mem.last_accessed_at = Some(days_ago_str(0));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(new_tier, Tier::Working);
    }

    #[test]
    fn test_promotion_to_core() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.access_count = 10;
        mem.importance = 0.9;
        mem.confidence = 0.9;
        mem.created_at = days_ago_str(1);
        mem.last_accessed_at = Some(days_ago_str(0));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(new_tier, Tier::Core);
    }

    #[test]
    fn test_demotion_to_peripheral() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.access_count = 1;
        mem.importance = 0.2;
        mem.confidence = 0.2;
        mem.created_at = days_ago_str(90);
        mem.last_accessed_at = None;

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Peripheral,
            "Old Working memory with low access should demote"
        );
    }

    #[test]
    fn test_no_change() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.access_count = 5;
        mem.importance = 0.6;
        mem.confidence = 0.6;
        mem.created_at = days_ago_str(15);
        mem.last_accessed_at = Some(days_ago_str(2));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Working,
            "Working memory meeting no threshold should stay Working"
        );
    }
}
