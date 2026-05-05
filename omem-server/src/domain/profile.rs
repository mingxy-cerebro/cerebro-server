use chrono::{DateTime, Utc};
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

/// Type of profile fact — affects confidence decay direction.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FactType {
    /// Static facts grow more confident over time (personality traits, values).
    Static,
    /// Dynamic facts decay over time (current habits, temporary states).
    Dynamic,
}

impl std::fmt::Display for FactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Static => write!(f, "static"),
            Self::Dynamic => write!(f, "dynamic"),
        }
    }
}

/// A single profile fact entry used for incremental updates.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfileFact {
    pub key: String,
    pub value: String,
    pub confidence: f32,
    pub fact_type: FactType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Tag prefix used to store profile fact keys in Memory.tags.
pub const PROFILE_FACT_TAG_PREFIX: &str = "pfact:";
/// Tag value for static facts.
pub const FACT_TYPE_STATIC_TAG: &str = "pfact_type:static";
/// Tag value for dynamic facts.
pub const FACT_TYPE_DYNAMIC_TAG: &str = "pfact_type:dynamic";

impl ProfileFact {
    /// Build the key tag: `pfact:{key}`.
    pub fn key_tag(&self) -> String {
        format!("{}{}", PROFILE_FACT_TAG_PREFIX, self.key)
    }

    /// Build the fact-type tag.
    pub fn fact_type_tag(&self) -> &'static str {
        match self.fact_type {
            FactType::Static => FACT_TYPE_STATIC_TAG,
            FactType::Dynamic => FACT_TYPE_DYNAMIC_TAG,
        }
    }
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

    #[test]
    fn fact_type_display_and_serde() {
        assert_eq!(FactType::Static.to_string(), "static");
        assert_eq!(FactType::Dynamic.to_string(), "dynamic");
        let json = serde_json::to_string(&FactType::Static).unwrap();
        assert_eq!(json, "\"static\"");
        let parsed: FactType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, FactType::Static);
    }

    #[test]
    fn profile_fact_key_tag_format() {
        let now = Utc::now();
        let fact = ProfileFact {
            key: "language".to_string(),
            value: "mandarin".to_string(),
            confidence: 0.8,
            fact_type: FactType::Static,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(fact.key_tag(), "pfact:language");
        assert_eq!(fact.fact_type_tag(), "pfact_type:static");

        let dynamic_fact = ProfileFact {
            fact_type: FactType::Dynamic,
            ..fact.clone()
        };
        assert_eq!(dynamic_fact.fact_type_tag(), "pfact_type:dynamic");
    }

    #[test]
    fn profile_fact_serde_roundtrip() {
        let now = Utc::now();
        let fact = ProfileFact {
            key: "ui_theme".to_string(),
            value: "dark mode".to_string(),
            confidence: 0.9,
            fact_type: FactType::Dynamic,
            created_at: now,
            updated_at: now,
        };
        let json = serde_json::to_string(&fact).unwrap();
        let parsed: ProfileFact = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.key, "ui_theme");
        assert_eq!(parsed.value, "dark mode");
        assert!((parsed.confidence - 0.9).abs() < f32::EPSILON);
        assert_eq!(parsed.fact_type, FactType::Dynamic);
    }
}
