use crate::domain::memory::Memory;
use crate::domain::types::Tier;

use super::decay::{parse_days_ago, DecayConfig, DecayEngine};

#[derive(Clone)]
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
            working_access_threshold: 6,
            working_composite_threshold: 0.4,
            core_access_threshold: 10,
            core_composite_threshold: 0.7,
            core_importance_threshold: 0.8,
            peripheral_composite_threshold: 0.2,
            peripheral_age_days: 90.0,
        }
    }
}

impl TierConfig {
    pub fn from_config(
        working_access_threshold: u32,
        working_composite_threshold: f32,
        core_access_threshold: u32,
        core_composite_threshold: f32,
        core_importance_threshold: f32,
        peripheral_composite_threshold: f32,
        peripheral_age_days: f32,
    ) -> Self {
        Self {
            working_access_threshold,
            working_composite_threshold,
            core_access_threshold,
            core_composite_threshold,
            core_importance_threshold,
            peripheral_composite_threshold,
            peripheral_age_days,
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

    pub fn from_config(tier_config: TierConfig, decay_config: DecayConfig) -> Self {
        Self::new(tier_config, DecayEngine::new(decay_config))
    }

    pub fn evaluate_tier(&self, memory: &Memory) -> Tier {
        let composite = self.decay.compute_composite(memory);
        let raw_composite = self.decay.compute_raw_composite(memory);
        let last_ref = memory
            .last_accessed_at
            .as_deref()
            .unwrap_or(&memory.created_at);
        let days_since_access = parse_days_ago(last_ref);

        // ponytail: importance随access_count渐进提升，打通Core升级路径
        // access_count=6→0.8（够Core门槛），=10→1.0封顶。纯函数不碰DB，13个caller全局生效
        let effective_importance = memory
            .importance
            .max(0.5 + memory.access_count as f32 * 0.05)
            .min(1.0);

        // 私密记忆不降级——敏感信息不应因长期未访问而遗忘
        let is_private = memory.visibility == "private";

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
                    && effective_importance >= self.config.core_importance_threshold
                {
                    Tier::Core
                } else if !is_private
                    && raw_composite < self.config.peripheral_composite_threshold
                    && days_since_access > self.config.peripheral_age_days
                {
                    Tier::Peripheral
                } else {
                    Tier::Working
                }
            }
            Tier::Core => {
                if !is_private
                    && raw_composite < self.config.peripheral_composite_threshold
                    && days_since_access > self.config.peripheral_age_days
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
            Category::new("preferences"),
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
        mem.access_count = 6;
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
    fn test_promotion_to_core_via_access_count() {
        // importance默认0.5但access_count高时，effective_importance应够Core门槛
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.access_count = 10;
        mem.importance = 0.5; // 默认值，但access_count=10→effective=1.0
        mem.confidence = 0.9;
        mem.created_at = days_ago_str(1);
        mem.last_accessed_at = Some(days_ago_str(0));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Core,
            "access_count=10 should push effective_importance to 1.0, clearing Core threshold"
        );
    }

    #[test]
    fn test_demotion_to_peripheral() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.access_count = 1;
        mem.importance = 0.2;
        mem.confidence = 0.2;
        mem.created_at = days_ago_str(100);
        mem.last_accessed_at = Some(days_ago_str(95));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Peripheral,
            "Old Working memory with low access AND long inactive should demote"
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

    #[test]
    fn test_demotion_by_raw_composite() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.access_count = 0;
        mem.importance = 0.05;
        mem.confidence = 0.05;
        mem.created_at = days_ago_str(120);
        mem.last_accessed_at = Some(days_ago_str(119));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Peripheral,
            "Working memory with very low raw composite should demote to Peripheral"
        );
    }

    #[test]
    fn test_core_demotion_to_working() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Core;
        mem.access_count = 1;
        mem.importance = 0.01;
        mem.confidence = 0.01;
        mem.created_at = days_ago_str(365);
        mem.last_accessed_at = Some(days_ago_str(360));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Working,
            "Core memory with low raw composite and low access should demote to Working"
        );
    }

    #[test]
    fn test_core_high_access_low_raw_stays_core() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Core;
        mem.access_count = 50;
        mem.importance = 0.01;
        mem.confidence = 0.01;
        mem.created_at = days_ago_str(365);
        mem.last_accessed_at = Some(days_ago_str(10));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Core,
            "Core memory with high access count and recent access should stay Core regardless of raw composite"
        );
    }

    #[test]
    fn test_core_demotes_when_long_inactive() {
        // 高importance+长期不访问：新AND逻辑下不降级（raw不会低）
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Core;
        mem.access_count = 50;
        mem.importance = 0.9;
        mem.confidence = 0.9;
        mem.created_at = days_ago_str(365);
        mem.last_accessed_at = Some(days_ago_str(120));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Core,
            "High-importance Core memory should not demote just by age (AND logic)"
        );
    }

    #[test]
    fn test_core_demotes_when_low_raw_and_long_inactive() {
        // 低importance+长期不访问：两个条件都满足才降级
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Core;
        mem.access_count = 0;
        mem.importance = 0.05;
        mem.confidence = 0.05;
        mem.created_at = days_ago_str(365);
        mem.last_accessed_at = Some(days_ago_str(120));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Working,
            "Low-raw Core memory not accessed for >90 days should demote to Working"
        );
    }

    #[test]
    fn test_working_demotes_when_long_inactive() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.access_count = 0;
        mem.importance = 0.1;
        mem.confidence = 0.1;
        mem.created_at = days_ago_str(120);
        mem.last_accessed_at = Some(days_ago_str(95));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Peripheral,
            "Working memory with low raw and >90 days inactive should demote to Peripheral"
        );
    }

    #[test]
    fn test_private_memory_never_demotes() {
        let manager = TierManager::with_defaults();

        let mut mem = make_test_memory();
        mem.tier = Tier::Working;
        mem.visibility = "private".to_string();
        mem.access_count = 5;
        mem.importance = 0.5;
        mem.confidence = 0.5;
        mem.created_at = days_ago_str(120);
        mem.last_accessed_at = Some(days_ago_str(95));

        let new_tier = manager.evaluate_tier(&mem);
        assert_eq!(
            new_tier,
            Tier::Working,
            "Private memory should never demote regardless of access pattern"
        );
    }
}
