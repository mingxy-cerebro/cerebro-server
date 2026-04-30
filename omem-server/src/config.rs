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
        }
    }
}

impl OmemConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
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
        }
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
