use chrono::{DateTime, Utc};

use crate::domain::memory::Memory;
use crate::domain::types::Tier;

pub struct DecayConfig {
    pub half_life_days: f32,
    pub recency_weight: f32,
    pub frequency_weight: f32,
    pub intrinsic_weight: f32,
    pub importance_modulation: f32,
    pub stale_threshold: f32,
    pub search_boost_min: f32,
    // Weibull β: Core<1 (sub-exp), Working=1, Peripheral>1 (super-exp)
    pub beta_core: f32,
    pub beta_working: f32,
    pub beta_peripheral: f32,
    pub floor_core: f32,
    pub floor_working: f32,
    pub floor_peripheral: f32,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            half_life_days: 30.0,
            recency_weight: 0.4,
            frequency_weight: 0.3,
            intrinsic_weight: 0.3,
            importance_modulation: 1.5,
            stale_threshold: 0.3,
            search_boost_min: 0.3,
            beta_core: 0.8,
            beta_working: 1.0,
            beta_peripheral: 1.3,
            floor_core: 0.9,
            floor_working: 0.7,
            floor_peripheral: 0.5,
        }
    }
}

pub struct DecayEngine {
    config: DecayConfig,
}

impl DecayEngine {
    pub fn new(config: DecayConfig) -> Self {
        Self { config }
    }

    // composite = w_r·recency + w_f·frequency + w_i·intrinsic, clamped to tier floor
    pub fn compute_composite(&self, memory: &Memory) -> f32 {
        let recency = self.compute_recency(memory);
        let frequency = self.compute_frequency(memory);
        let intrinsic = self.compute_intrinsic(memory);

        let raw = self.config.recency_weight * recency
            + self.config.frequency_weight * frequency
            + self.config.intrinsic_weight * intrinsic;

        raw.max(self.get_floor(&memory.tier))
    }

    // boosted = score · (min_boost + (1 - min_boost) · composite)
    pub fn apply_search_boost(&self, score: f32, memory: &Memory) -> f32 {
        let composite = self.compute_composite(memory);
        let factor =
            self.config.search_boost_min + (1.0 - self.config.search_boost_min) * composite;
        score * factor
    }

    pub fn is_stale(&self, memory: &Memory) -> bool {
        self.compute_composite(memory) < self.config.stale_threshold
    }

    // Weibull: recency = exp(-λ·t^β), λ = ln2/hl_eff, hl_eff = hl·exp(μ·importance)
    fn compute_recency(&self, memory: &Memory) -> f32 {
        let effective_hl = self.config.half_life_days
            * (self.config.importance_modulation * memory.importance).exp();
        let lambda = 2.0_f32.ln() / effective_hl;
        let days_since = self.days_since_last_access(memory);
        let beta = self.get_beta(&memory.tier);
        (-lambda * days_since.powf(beta)).exp()
    }

    // base = 1 - exp(-count/5); if count>1 modulate by avg-gap recentness
    fn compute_frequency(&self, memory: &Memory) -> f32 {
        let base = 1.0 - (-(memory.access_count as f32) / 5.0).exp();
        if memory.access_count > 1 {
            let avg_gap = self.compute_avg_gap_days(memory);
            let recentness_bonus = (-avg_gap / 30.0).exp();
            base * (0.5 + 0.5 * recentness_bonus)
        } else {
            base
        }
    }

    fn compute_intrinsic(&self, memory: &Memory) -> f32 {
        memory.importance * memory.confidence
    }

    fn get_beta(&self, tier: &Tier) -> f32 {
        match tier {
            Tier::Core => self.config.beta_core,
            Tier::Working => self.config.beta_working,
            Tier::Peripheral => self.config.beta_peripheral,
        }
    }

    fn get_floor(&self, tier: &Tier) -> f32 {
        match tier {
            Tier::Core => self.config.floor_core,
            Tier::Working => self.config.floor_working,
            Tier::Peripheral => self.config.floor_peripheral,
        }
    }

    fn days_since_last_access(&self, memory: &Memory) -> f32 {
        let reference = memory
            .last_accessed_at
            .as_deref()
            .unwrap_or(&memory.created_at);
        parse_days_ago(reference)
    }

    fn compute_avg_gap_days(&self, memory: &Memory) -> f32 {
        if memory.access_count <= 1 {
            return 0.0;
        }
        let created = parse_datetime(&memory.created_at);
        let last = memory
            .last_accessed_at
            .as_deref()
            .and_then(parse_datetime)
            .or_else(|| parse_datetime(&memory.created_at));

        match (created, last) {
            (Some(c), Some(l)) => {
                let span_days = (l - c).num_seconds() as f32 / 86400.0;
                span_days.max(0.0) / (memory.access_count - 1) as f32
            }
            _ => 0.0,
        }
    }
}

pub(crate) fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub(crate) fn parse_days_ago(s: &str) -> f32 {
    match parse_datetime(s) {
        Some(dt) => {
            let dur = Utc::now() - dt;
            (dur.num_seconds() as f32 / 86400.0).max(0.0)
        }
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::Category;
    use crate::domain::types::MemoryType;

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
    fn test_core_decays_slowly() {
        let engine = DecayEngine::new(DecayConfig::default());
        let mut mem = make_test_memory();
        mem.tier = Tier::Core;
        mem.access_count = 20;
        mem.importance = 0.9;
        mem.confidence = 0.9;
        mem.created_at = days_ago_str(60);
        mem.last_accessed_at = Some(days_ago_str(10));

        let composite = engine.compute_composite(&mem);
        assert!(
            composite >= 0.9,
            "Core memory composite should be >= 0.9 (floor), got {composite}"
        );
    }

    #[test]
    fn test_peripheral_decays_fast() {
        let engine = DecayEngine::new(DecayConfig::default());
        let mut mem = make_test_memory();
        mem.tier = Tier::Peripheral;
        mem.access_count = 1;
        mem.importance = 0.3;
        mem.confidence = 0.3;
        mem.created_at = days_ago_str(90);
        mem.last_accessed_at = None;

        let composite = engine.compute_composite(&mem);
        assert!(
            (composite - 0.5).abs() < 0.05,
            "Old Peripheral memory should be at floor 0.5, got {composite}"
        );
        assert!(
            composite < 0.9,
            "Peripheral should be much lower than Core floor"
        );
    }

    #[test]
    fn test_importance_modulates_halflife() {
        let engine = DecayEngine::new(DecayConfig {
            floor_peripheral: 0.0,
            ..DecayConfig::default()
        });

        let mut low_imp = make_test_memory();
        low_imp.tier = Tier::Peripheral;
        low_imp.importance = 0.1;
        low_imp.confidence = 0.5;
        low_imp.access_count = 1;
        low_imp.created_at = days_ago_str(30);
        low_imp.last_accessed_at = None;

        let mut high_imp = low_imp.clone();
        high_imp.importance = 0.9;

        let low_composite = engine.compute_composite(&low_imp);
        let high_composite = engine.compute_composite(&high_imp);

        assert!(
            high_composite > low_composite,
            "Higher importance → higher composite: high={high_composite} low={low_composite}"
        );
    }

    #[test]
    fn test_frequency_saturates() {
        let engine = DecayEngine::new(DecayConfig {
            recency_weight: 0.0,
            frequency_weight: 1.0,
            intrinsic_weight: 0.0,
            floor_peripheral: 0.0,
            ..DecayConfig::default()
        });

        let mut mem5 = make_test_memory();
        mem5.tier = Tier::Peripheral;
        mem5.access_count = 5;
        mem5.created_at = days_ago_str(30);
        mem5.last_accessed_at = Some(days_ago_str(1));

        let mut mem100 = mem5.clone();
        mem100.access_count = 100;

        let freq5 = engine.compute_composite(&mem5);
        let freq100 = engine.compute_composite(&mem100);

        assert!(
            freq100 > freq5,
            "100 accesses should score higher than 5: f100={freq100} f5={freq5}"
        );
        assert!(
            freq100 > 0.9,
            "100 accesses should saturate near 1.0, got {freq100}"
        );
    }

    #[test]
    fn test_stale_detection() {
        let engine = DecayEngine::new(DecayConfig {
            floor_peripheral: 0.0,
            ..DecayConfig::default()
        });

        let mut mem = make_test_memory();
        mem.tier = Tier::Peripheral;
        mem.access_count = 0;
        mem.importance = 0.1;
        mem.confidence = 0.1;
        mem.created_at = days_ago_str(365);
        mem.last_accessed_at = None;

        assert!(
            engine.is_stale(&mem),
            "Very old, never-accessed low-importance memory should be stale"
        );

        let fresh = make_test_memory();
        assert!(!engine.is_stale(&fresh), "Fresh memory should not be stale");
    }

    #[test]
    fn test_search_boost() {
        let engine = DecayEngine::new(DecayConfig::default());
        let mem = make_test_memory();

        let base_score = 0.8;
        let boosted = engine.apply_search_boost(base_score, &mem);
        assert!(
            boosted >= base_score * 0.3,
            "Boosted should be at least min_boost * score"
        );
        assert!(
            boosted <= base_score + f32::EPSILON,
            "Boosted should not exceed original score"
        );
    }

    #[test]
    fn test_default_config_weights_sum_to_one() {
        let cfg = DecayConfig::default();
        let total = cfg.recency_weight + cfg.frequency_weight + cfg.intrinsic_weight;
        assert!(
            (total - 1.0).abs() < f32::EPSILON,
            "Weights should sum to 1.0, got {total}"
        );
    }
}
