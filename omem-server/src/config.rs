use std::env;

#[derive(Debug, Clone)]
pub struct OmemConfig {
    pub port: u16,
    pub log_level: String,
    pub s3_bucket: String,
    pub oss_bucket: String,
    pub embed_provider: String,
    pub llm_provider: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub llm_base_url: String,
    pub llm_response_format: Option<String>,
    pub embed_api_key: String,
    pub embed_base_url: String,
    pub embed_model: String,
    pub recall_llm_provider: String,
    pub recall_llm_api_key: String,
    pub recall_llm_model: String,
    pub recall_llm_base_url: String,

    // Cluster LLM (cheaper model for clustering/profile tasks, e.g. Qwen3.5-4B via SiliconFlow)
    pub cluster_llm_provider: String,
    pub cluster_llm_api_key: String,
    pub cluster_llm_model: String,
    pub cluster_llm_base_url: String,

    pub scheduler_interval_secs: u64,
    pub scheduler_run_on_start: bool,

    // Rate limiting
    /// Minimum seconds between should-recall calls per tenant. Default: 30
    pub should_recall_min_interval_secs: u64,

    // Admission control
    /// Admission preset: balanced, conservative, high_recall. Default: high_recall
    pub admission_preset: String,
    /// Custom admission reject threshold (overrides preset). Default: None
    pub admission_reject_threshold: Option<f32>,
    /// Custom admission admit threshold (overrides preset). Default: None
    pub admission_admit_threshold: Option<f32>,

    // Clustering configuration
    /// Minimum similarity score (0.0-1.0) for a memory to be considered for cluster assignment.
    /// Memories with similarity below this threshold will create new clusters.
    /// Default: 0.75
    pub cluster_similarity_threshold: f32,
    
    /// Similarity score (0.0-1.0) above which memories are automatically assigned to existing clusters
    /// without LLM judgment. Higher values = more conservative assignments.
    /// Default: 0.90
    pub cluster_auto_merge_threshold: f32,
    
    /// Number of candidate clusters to search for when assigning a memory.
    /// Higher values = more thorough search but slower.
    /// Default: 5
    pub cluster_candidate_count: usize,
    
    /// Whether to use LLM for judging cluster assignments in the ambiguous range
    /// (between similarity_threshold and auto_merge_threshold).
    /// Set to "false" to disable LLM judgment and use similarity scores only.
    /// Default: true
    pub cluster_llm_judge_enabled: bool,

    // Lifecycle decay configuration
    pub decay_half_life_days: f32,
    pub decay_stale_threshold: f32,
    pub decay_importance_modulation: f32,
    pub decay_beta_core: f32,
    pub decay_beta_working: f32,
    pub decay_beta_peripheral: f32,
    pub decay_floor_core: f32,
    pub decay_floor_working: f32,
    pub decay_floor_peripheral: f32,

    // Tier configuration
    pub tier_working_access_threshold: u32,
    pub tier_working_composite_threshold: f32,
    pub tier_core_access_threshold: u32,
    pub tier_core_composite_threshold: f32,
    pub tier_core_importance_threshold: f32,
    pub tier_peripheral_composite_threshold: f32,
    pub tier_peripheral_age_days: f32,

    // Forgetting configuration
    pub forgetting_max_stale_deletions: usize,
    pub forgetting_access_count_protection: u32,
    pub forgetting_superseded_archive_days: u32,
}

impl Default for OmemConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            log_level: "info".to_string(),
            s3_bucket: String::new(),
            oss_bucket: String::new(),
            embed_provider: "noop".to_string(),
            llm_provider: String::new(),
            llm_api_key: String::new(),
            llm_model: "gpt-4o-mini".to_string(),
            llm_base_url: "https://api.openai.com".to_string(),
            llm_response_format: None,
            embed_api_key: String::new(),
            embed_base_url: String::new(),
            embed_model: String::new(),
            recall_llm_provider: String::new(),
            recall_llm_api_key: String::new(),
            recall_llm_model: String::new(),
            recall_llm_base_url: String::new(),
            cluster_llm_provider: String::new(),
            cluster_llm_api_key: String::new(),
            cluster_llm_model: String::new(),
            cluster_llm_base_url: String::new(),
            scheduler_interval_secs: 21600,
            scheduler_run_on_start: true,
            should_recall_min_interval_secs: 30,
            admission_preset: "high_recall".to_string(),
            admission_reject_threshold: None,
            admission_admit_threshold: None,
            cluster_similarity_threshold: 0.55,
            cluster_auto_merge_threshold: 0.90,
            cluster_candidate_count: 15,
            cluster_llm_judge_enabled: true,
            decay_half_life_days: 30.0,
            decay_stale_threshold: 0.3,
            decay_importance_modulation: 1.5,
            decay_beta_core: 0.8,
            decay_beta_working: 1.0,
            decay_beta_peripheral: 1.3,
            decay_floor_core: 0.9,
            decay_floor_working: 0.7,
            decay_floor_peripheral: 0.5,
            tier_working_access_threshold: 3,
            tier_working_composite_threshold: 0.4,
            tier_core_access_threshold: 10,
            tier_core_composite_threshold: 0.7,
            tier_core_importance_threshold: 0.8,
            tier_peripheral_composite_threshold: 0.15,
            tier_peripheral_age_days: 60.0,
            forgetting_max_stale_deletions: 50,
            forgetting_access_count_protection: 5,
            forgetting_superseded_archive_days: 30,
        }
    }
}

impl OmemConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        let mut config = Self {
            port: env::var("OMEM_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.port),
            log_level: env::var("OMEM_LOG_LEVEL").unwrap_or(defaults.log_level),
            s3_bucket: env::var("OMEM_S3_BUCKET").unwrap_or(defaults.s3_bucket),
            oss_bucket: env::var("OMEM_OSS_BUCKET").unwrap_or(defaults.oss_bucket),
            embed_provider: env::var("OMEM_EMBED_PROVIDER").unwrap_or(defaults.embed_provider),
            llm_provider: env::var("OMEM_LLM_PROVIDER").unwrap_or(defaults.llm_provider),
            llm_api_key: env::var("OMEM_LLM_API_KEY").unwrap_or(defaults.llm_api_key),
            llm_model: env::var("OMEM_LLM_MODEL").unwrap_or(defaults.llm_model),
            llm_base_url: env::var("OMEM_LLM_BASE_URL").unwrap_or(defaults.llm_base_url),
            llm_response_format: env::var("OMEM_LLM_RESPONSE_FORMAT")
                .ok()
                .filter(|v| !v.is_empty()),
            embed_api_key: env::var("OMEM_EMBED_API_KEY").unwrap_or(defaults.embed_api_key),
            embed_base_url: env::var("OMEM_EMBED_BASE_URL").unwrap_or(defaults.embed_base_url),
            embed_model: env::var("OMEM_EMBED_MODEL").unwrap_or(defaults.embed_model),
            recall_llm_provider: env::var("OMEM_RECALL_LLM_PROVIDER").unwrap_or(defaults.recall_llm_provider),
            recall_llm_api_key: env::var("OMEM_RECALL_LLM_API_KEY").unwrap_or(defaults.recall_llm_api_key),
            recall_llm_model: env::var("OMEM_RECALL_LLM_MODEL").unwrap_or(defaults.recall_llm_model),
            recall_llm_base_url: env::var("OMEM_RECALL_LLM_BASE_URL").unwrap_or(defaults.recall_llm_base_url),
            cluster_llm_provider: env::var("OMEM_CLUSTER_LLM_PROVIDER").unwrap_or(defaults.cluster_llm_provider),
            cluster_llm_api_key: env::var("OMEM_CLUSTER_LLM_API_KEY").unwrap_or(defaults.cluster_llm_api_key),
            cluster_llm_model: env::var("OMEM_CLUSTER_LLM_MODEL").unwrap_or(defaults.cluster_llm_model),
            cluster_llm_base_url: env::var("OMEM_CLUSTER_LLM_BASE_URL").unwrap_or(defaults.cluster_llm_base_url),
            scheduler_interval_secs: env::var("OMEM_SCHEDULER_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.scheduler_interval_secs),
            scheduler_run_on_start: env::var("OMEM_SCHEDULER_RUN_ON_START")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.scheduler_run_on_start),
            should_recall_min_interval_secs: env::var("OMEM_SHOULD_RECALL_MIN_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.should_recall_min_interval_secs),
            admission_preset: env::var("OMEM_ADMISSION_PRESET")
                .unwrap_or(defaults.admission_preset),
            admission_reject_threshold: env::var("OMEM_ADMISSION_REJECT_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok()),
            admission_admit_threshold: env::var("OMEM_ADMISSION_ADMIT_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok()),
            cluster_similarity_threshold: env::var("OMEM_CLUSTER_SIMILARITY_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.cluster_similarity_threshold),
            cluster_auto_merge_threshold: env::var("OMEM_CLUSTER_AUTO_MERGE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.cluster_auto_merge_threshold),
            cluster_candidate_count: env::var("OMEM_CLUSTER_CANDIDATE_COUNT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.cluster_candidate_count),
            cluster_llm_judge_enabled: env::var("OMEM_CLUSTER_LLM_JUDGE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.cluster_llm_judge_enabled),
            decay_half_life_days: env::var("OMEM_DECAY_HALF_LIFE_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_half_life_days),
            decay_stale_threshold: env::var("OMEM_DECAY_STALE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_stale_threshold),
            decay_importance_modulation: env::var("OMEM_DECAY_IMPORTANCE_MODULATION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_importance_modulation),
            decay_beta_core: env::var("OMEM_DECAY_BETA_CORE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_beta_core),
            decay_beta_working: env::var("OMEM_DECAY_BETA_WORKING")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_beta_working),
            decay_beta_peripheral: env::var("OMEM_DECAY_BETA_PERIPHERAL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_beta_peripheral),
            decay_floor_core: env::var("OMEM_DECAY_FLOOR_CORE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_floor_core),
            decay_floor_working: env::var("OMEM_DECAY_FLOOR_WORKING")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_floor_working),
            decay_floor_peripheral: env::var("OMEM_DECAY_FLOOR_PERIPHERAL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.decay_floor_peripheral),
            tier_working_access_threshold: env::var("OMEM_TIER_WORKING_ACCESS_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.tier_working_access_threshold),
            tier_working_composite_threshold: env::var("OMEM_TIER_WORKING_COMPOSITE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.tier_working_composite_threshold),
            tier_core_access_threshold: env::var("OMEM_TIER_CORE_ACCESS_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.tier_core_access_threshold),
            tier_core_composite_threshold: env::var("OMEM_TIER_CORE_COMPOSITE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.tier_core_composite_threshold),
            tier_core_importance_threshold: env::var("OMEM_TIER_CORE_IMPORTANCE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.tier_core_importance_threshold),
            tier_peripheral_composite_threshold: env::var("OMEM_TIER_PERIPHERAL_COMPOSITE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.tier_peripheral_composite_threshold),
            tier_peripheral_age_days: env::var("OMEM_TIER_PERIPHERAL_AGE_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.tier_peripheral_age_days),
            forgetting_max_stale_deletions: env::var("OMEM_FORGETTING_MAX_STALE_DELETIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.forgetting_max_stale_deletions),
            forgetting_access_count_protection: env::var("OMEM_FORGETTING_ACCESS_COUNT_PROTECTION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.forgetting_access_count_protection),
            forgetting_superseded_archive_days: env::var("OMEM_FORGETTING_SUPERSEDED_ARCHIVE_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.forgetting_superseded_archive_days),
        };

        // Validate lifecycle parameters — warn and fallback to defaults on invalid values
        let d = OmemConfig::default();
        if config.decay_half_life_days <= 0.0 || config.decay_half_life_days.is_nan() {
            tracing::warn!("OMEM_DECAY_HALF_LIFE_DAYS must be > 0, falling back to default");
            config.decay_half_life_days = d.decay_half_life_days;
        }
        if config.decay_stale_threshold <= 0.0 || config.decay_stale_threshold > 1.0 {
            tracing::warn!("OMEM_DECAY_STALE_THRESHOLD must be in (0, 1], falling back to default");
            config.decay_stale_threshold = d.decay_stale_threshold;
        }
        if config.decay_importance_modulation <= 0.0 || config.decay_importance_modulation.is_nan() {
            tracing::warn!("OMEM_DECAY_IMPORTANCE_MODULATION must be > 0, falling back to default");
            config.decay_importance_modulation = d.decay_importance_modulation;
        }
        if config.decay_beta_core <= 0.0 || config.decay_beta_core.is_nan() {
            tracing::warn!("OMEM_DECAY_BETA_CORE must be > 0, falling back to default");
            config.decay_beta_core = d.decay_beta_core;
        }
        if config.decay_beta_working <= 0.0 || config.decay_beta_working.is_nan() {
            tracing::warn!("OMEM_DECAY_BETA_WORKING must be > 0, falling back to default");
            config.decay_beta_working = d.decay_beta_working;
        }
        if config.decay_beta_peripheral <= 0.0 || config.decay_beta_peripheral.is_nan() {
            tracing::warn!("OMEM_DECAY_BETA_PERIPHERAL must be > 0, falling back to default");
            config.decay_beta_peripheral = d.decay_beta_peripheral;
        }
        if !(0.0..=1.0).contains(&config.decay_floor_core) {
            tracing::warn!("OMEM_DECAY_FLOOR_CORE must be in [0, 1], falling back to default");
            config.decay_floor_core = d.decay_floor_core;
        }
        if !(0.0..=1.0).contains(&config.decay_floor_working) {
            tracing::warn!("OMEM_DECAY_FLOOR_WORKING must be in [0, 1], falling back to default");
            config.decay_floor_working = d.decay_floor_working;
        }
        if !(0.0..=1.0).contains(&config.decay_floor_peripheral) {
            tracing::warn!("OMEM_DECAY_FLOOR_PERIPHERAL must be in [0, 1], falling back to default");
            config.decay_floor_peripheral = d.decay_floor_peripheral;
        }
        if config.tier_working_composite_threshold <= 0.0 || config.tier_working_composite_threshold > 1.0 {
            tracing::warn!("OMEM_TIER_WORKING_COMPOSITE_THRESHOLD must be in (0, 1], falling back to default");
            config.tier_working_composite_threshold = d.tier_working_composite_threshold;
        }
        if config.tier_core_composite_threshold <= 0.0 || config.tier_core_composite_threshold > 1.0 {
            tracing::warn!("OMEM_TIER_CORE_COMPOSITE_THRESHOLD must be in (0, 1], falling back to default");
            config.tier_core_composite_threshold = d.tier_core_composite_threshold;
        }
        if config.tier_core_importance_threshold <= 0.0 || config.tier_core_importance_threshold > 1.0 {
            tracing::warn!("OMEM_TIER_CORE_IMPORTANCE_THRESHOLD must be in (0, 1], falling back to default");
            config.tier_core_importance_threshold = d.tier_core_importance_threshold;
        }
        if config.tier_peripheral_composite_threshold <= 0.0 || config.tier_peripheral_composite_threshold > 1.0 {
            tracing::warn!("OMEM_TIER_PERIPHERAL_COMPOSITE_THRESHOLD must be in (0, 1], falling back to default");
            config.tier_peripheral_composite_threshold = d.tier_peripheral_composite_threshold;
        }
        if config.tier_peripheral_age_days <= 0.0 {
            tracing::warn!("OMEM_TIER_PERIPHERAL_AGE_DAYS must be > 0, falling back to default");
            config.tier_peripheral_age_days = d.tier_peripheral_age_days;
        }
        if config.forgetting_max_stale_deletions == 0 {
            tracing::warn!("OMEM_FORGETTING_MAX_STALE_DELETIONS must be > 0, falling back to default");
            config.forgetting_max_stale_deletions = d.forgetting_max_stale_deletions;
        }
        if config.forgetting_superseded_archive_days == 0 {
            tracing::warn!("OMEM_FORGETTING_SUPERSEDED_ARCHIVE_DAYS must be > 0, falling back to default");
            config.forgetting_superseded_archive_days = d.forgetting_superseded_archive_days;
        }

        config
    }

    pub fn decay_config(&self) -> crate::lifecycle::decay::DecayConfig {
        crate::lifecycle::decay::DecayConfig::from_config(
            self.decay_half_life_days,
            self.decay_stale_threshold,
            self.decay_importance_modulation,
            self.decay_beta_core,
            self.decay_beta_working,
            self.decay_beta_peripheral,
            self.decay_floor_core,
            self.decay_floor_working,
            self.decay_floor_peripheral,
        )
    }

    pub fn tier_config(&self) -> crate::lifecycle::tier::TierConfig {
        crate::lifecycle::tier::TierConfig::from_config(
            self.tier_working_access_threshold,
            self.tier_working_composite_threshold,
            self.tier_core_access_threshold,
            self.tier_core_composite_threshold,
            self.tier_core_importance_threshold,
            self.tier_peripheral_composite_threshold,
            self.tier_peripheral_age_days,
        )
    }

    pub fn store_uri(&self) -> String {
        if !self.oss_bucket.is_empty() {
            format!("oss://{}/omem", self.oss_bucket)
        } else if !self.s3_bucket.is_empty() {
            format!("s3://{}/omem", self.s3_bucket)
        } else {
            "./omem-data".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_valid() {
        let config = OmemConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.embed_provider, "noop");
        assert_eq!(config.llm_model, "gpt-4o-mini");
        assert!(config.llm_response_format.is_none());
        assert_eq!(config.log_level, "info");
    }
}
