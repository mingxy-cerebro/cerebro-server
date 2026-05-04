use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct StaticFact {
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub visibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l2_content: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct UserProfile {
    pub static_facts: Vec<StaticFact>,
    pub dynamic_context: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_profile_serde_roundtrip() {
        let profile = UserProfile {
            static_facts: vec![
                StaticFact {
                    content: "speaks mandarin".to_string(),
                    tags: vec!["language".to_string()],
                    visibility: "personal".to_string(),
                    l2_content: Some("详细内容".to_string()),
                },
                StaticFact {
                    content: "rust developer".to_string(),
                    tags: vec![],
                    visibility: "team".to_string(),
                    l2_content: None,
                },
            ],
            dynamic_context: vec!["working on omem project".to_string()],
        };
        let json = serde_json::to_string(&profile).unwrap();
        let parsed: UserProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.static_facts.len(), 2);
        assert_eq!(parsed.static_facts[0].content, "speaks mandarin");
        assert_eq!(parsed.static_facts[0].tags[0], "language");
        assert_eq!(parsed.static_facts[1].visibility, "team");
        assert_eq!(parsed.dynamic_context[0], "working on omem project");
    }

    #[test]
    fn user_profile_default_empty() {
        let profile = UserProfile::default();
        assert!(profile.static_facts.is_empty());
        assert!(profile.dynamic_context.is_empty());
    }
}
