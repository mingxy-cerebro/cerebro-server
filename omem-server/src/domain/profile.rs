use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct UserProfile {
    pub static_facts: Vec<String>,
    pub dynamic_context: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_profile_serde_roundtrip() {
        let profile = UserProfile {
            static_facts: vec!["speaks mandarin".to_string(), "rust developer".to_string()],
            dynamic_context: vec!["working on omem project".to_string()],
        };
        let json = serde_json::to_string(&profile).unwrap();
        let parsed: UserProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.static_facts.len(), 2);
        assert_eq!(parsed.dynamic_context[0], "working on omem project");
    }

    #[test]
    fn user_profile_default_empty() {
        let profile = UserProfile::default();
        assert!(profile.static_facts.is_empty());
        assert!(profile.dynamic_context.is_empty());
    }
}
