use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MemoryRelation {
    pub relation_type: RelationType,
    pub target_id: String,
    pub context_label: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RelationType {
    Supersedes,
    Contextualizes,
    Supports,
    Contradicts,
    Continues,
    ContinuedBy,
}

impl fmt::Display for RelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Supersedes => write!(f, "supersedes"),
            Self::Contextualizes => write!(f, "contextualizes"),
            Self::Supports => write!(f, "supports"),
            Self::Contradicts => write!(f, "contradicts"),
            Self::Continues => write!(f, "continues"),
            Self::ContinuedBy => write!(f, "continued_by"),
        }
    }
}

impl FromStr for RelationType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "supersedes" => Ok(Self::Supersedes),
            "contextualizes" => Ok(Self::Contextualizes),
            "supports" => Ok(Self::Supports),
            "contradicts" => Ok(Self::Contradicts),
            "continues" => Ok(Self::Continues),
            "continued_by" => Ok(Self::ContinuedBy),
            _ => Err(format!("unknown relation type: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_type_display_lowercase() {
        assert_eq!(RelationType::Supersedes.to_string(), "supersedes");
        assert_eq!(RelationType::Contextualizes.to_string(), "contextualizes");
        assert_eq!(RelationType::Supports.to_string(), "supports");
        assert_eq!(RelationType::Contradicts.to_string(), "contradicts");
        assert_eq!(RelationType::Continues.to_string(), "continues");
        assert_eq!(RelationType::ContinuedBy.to_string(), "continued_by");
    }

    #[test]
    fn continues_from_str_roundtrip() {
        let rt: RelationType = "continues".parse().unwrap();
        assert_eq!(rt, RelationType::Continues);
        assert_eq!(rt.to_string(), "continues");
    }

    #[test]
    fn continues_serde_roundtrip() {
        let rel = MemoryRelation {
            relation_type: RelationType::Continues,
            target_id: "mem-789".to_string(),
            context_label: Some("split continuation".to_string()),
        };
        let json = serde_json::to_string(&rel).unwrap();
        let parsed: MemoryRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rel);
        assert!(json.contains("\"continues\""));
    }

    #[test]
    fn continued_by_from_str_roundtrip() {
        let rt: RelationType = "continued_by".parse().unwrap();
        assert_eq!(rt, RelationType::ContinuedBy);
        assert_eq!(rt.to_string(), "continued_by");
    }

    #[test]
    fn continued_by_serde_roundtrip() {
        let rel = MemoryRelation {
            relation_type: RelationType::ContinuedBy,
            target_id: "mem-789".to_string(),
            context_label: Some("auto-split continuation".to_string()),
        };
        let json = serde_json::to_string(&rel).unwrap();
        let parsed: MemoryRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rel);
        assert!(json.contains("\"continued_by\""));
    }

    #[test]
    fn memory_relation_serde_roundtrip() {
        let rel = MemoryRelation {
            relation_type: RelationType::Supersedes,
            target_id: "mem-123".to_string(),
            context_label: Some("updated preference".to_string()),
        };
        let json = serde_json::to_string(&rel).unwrap();
        let parsed: MemoryRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rel);
    }

    #[test]
    fn memory_relation_without_context_label() {
        let rel = MemoryRelation {
            relation_type: RelationType::Supports,
            target_id: "mem-456".to_string(),
            context_label: None,
        };
        let json = serde_json::to_string(&rel).unwrap();
        let parsed: MemoryRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rel);
    }
}
