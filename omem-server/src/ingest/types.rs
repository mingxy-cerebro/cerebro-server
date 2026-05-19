use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExtractedFact {
    pub l0_abstract: String,
    pub l1_overview: String,
    pub l2_content: String,
    pub category: String,
    pub tags: Vec<String>,
    #[serde(skip)]
    pub source_text: Option<String>,
    /// Content quality score computed from structural features (0.0-1.0).
    /// Used to set initial memory confidence on creation.
    #[serde(default)]
    pub quality_score: f32,
    #[serde(default = "default_visibility")]
    pub visibility: String,
    #[serde(default)]
    pub owner_agent_id: String,
    /// LLM-reported confidence that this fact is worth remembering (1-5, where 5 = very high value).
    /// 0 means the field was not provided by the LLM.
    #[serde(default)]
    pub llm_confidence: u8,
}

fn default_visibility() -> String {
    "global".to_string()
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtractionResult {
    pub memories: Vec<ExtractedFact>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReconcileDecision {
    pub action: String,
    pub fact_index: usize,
    #[serde(default)]
    pub match_index: Option<usize>,
    #[serde(default)]
    pub merged_content: Option<String>,
    #[serde(default)]
    pub context_label: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReconcileResult {
    pub decisions: Vec<ReconcileDecision>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BatchDedupResult {
    pub keep_indices: Vec<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IngestMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IngestRequest {
    pub messages: Vec<IngestMessage>,
    pub tenant_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub entity_context: Option<String>,
    #[serde(default)]
    pub mode: IngestMode,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IngestMode {
    #[default]
    Smart,
    Raw,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IngestResponse {
    pub task_id: String,
    pub stored_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ingest_request_parse_project_path() {
        let json = r#"{
            "messages": [{"role": "user", "content": "hello"}],
            "tenant_id": "t-001",
            "project_path": "/home/user/my-project"
        }"#;
        let req: IngestRequest = serde_json::from_str(json).expect("parse");
        assert_eq!(req.project_path.as_deref(), Some("/home/user/my-project"));
    }

    #[test]
    fn test_ingest_request_project_path_default_none() {
        let json = r#"{
            "messages": [{"role": "user", "content": "hello"}],
            "tenant_id": "t-001"
        }"#;
        let req: IngestRequest = serde_json::from_str(json).expect("parse");
        assert!(req.project_path.is_none());
    }

    #[test]
    fn test_ingest_request_project_path_empty_string() {
        let json = r#"{
            "messages": [{"role": "user", "content": "hello"}],
            "tenant_id": "t-001",
            "project_path": ""
        }"#;
        let req: IngestRequest = serde_json::from_str(json).expect("parse");
        // Empty string is preserved as Some("") in the request; normalization
        // to None happens in create_fact_memory() at write time.
        assert_eq!(req.project_path.as_deref(), Some(""));
    }
}
