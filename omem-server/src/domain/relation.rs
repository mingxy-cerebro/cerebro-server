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
}

impl fmt::Display for RelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Supersedes => write!(f, "supersedes"),
            Self::Contextualizes => write!(f, "contextualizes"),
            Self::Supports => write!(f, "supports"),
            Self::Contradicts => write!(f, "contradicts"),
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
