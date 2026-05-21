use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceStatus {
    Active,
    Reinforce,
    Dormant,
    Deleted,
}

impl PreferenceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Reinforce => "reinforce",
            Self::Dormant => "dormant",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceScope {
    Project,
    Global,
}

impl PreferenceScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preference {
    pub id: String,
    pub tenant_id: String,
    pub slot: String,
    pub value: String,
    pub confidence: f32,
    pub scope: PreferenceScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    pub source: String,
    pub status: PreferenceStatus,
    pub last_reinforced_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InductionRun {
    pub id: String,
    pub tenant_id: String,
    pub status: String,
    pub candidate_count: i32,
    pub extracted_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InductionLock {
    pub id: String,
    pub tenant_id: String,
    pub created_at: DateTime<Utc>,
    pub ttl_secs: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileVersion {
    pub id: String,
    pub tenant_id: String,
    pub snapshot: String,
    pub preference_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileChangelog {
    pub id: String,
    pub tenant_id: String,
    pub preference_id: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_value: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionRequest {
    pub tenant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionResult {
    pub content: String,
    pub preference_count: i32,
    pub estimated_tokens: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InductedPreference {
    pub slot: String,
    pub value: String,
    pub confidence: f32,
    pub scope: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_status_roundtrip() {
        let status = PreferenceStatus::Active;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"active\"");
        let parsed: PreferenceStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn preference_scope_roundtrip() {
        let scope = PreferenceScope::Global;
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, "\"global\"");
        let parsed: PreferenceScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }

    #[test]
    fn preference_serde_roundtrip() {
        let pref = Preference {
            id: "test-id".to_string(),
            tenant_id: "tenant-1".to_string(),
            slot: "language".to_string(),
            value: "中文".to_string(),
            confidence: 0.8,
            scope: PreferenceScope::Global,
            project_path: None,
            source: "observed".to_string(),
            status: PreferenceStatus::Active,
            last_reinforced_at: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&pref).unwrap();
        let parsed: Preference = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, pref.id);
        assert_eq!(parsed.slot, pref.slot);
    }
}
