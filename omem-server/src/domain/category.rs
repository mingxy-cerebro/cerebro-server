use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Profile,
    Preferences,
    Entities,
    Events,
    Cases,
    Patterns,
}

impl Category {
    pub fn is_always_merge(&self) -> bool {
        matches!(self, Self::Profile)
    }

    pub fn is_append_only(&self) -> bool {
        matches!(self, Self::Events | Self::Cases)
    }

    pub fn is_temporal_versioned(&self) -> bool {
        matches!(self, Self::Preferences | Self::Entities)
    }

    pub fn is_merge_supported(&self) -> bool {
        matches!(self, Self::Preferences | Self::Entities | Self::Patterns)
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile => write!(f, "profile"),
            Self::Preferences => write!(f, "preferences"),
            Self::Entities => write!(f, "entities"),
            Self::Events => write!(f, "events"),
            Self::Cases => write!(f, "cases"),
            Self::Patterns => write!(f, "patterns"),
        }
    }
}

impl FromStr for Category {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "profile" => Ok(Self::Profile),
            "preferences" => Ok(Self::Preferences),
            "entities" => Ok(Self::Entities),
            "events" => Ok(Self::Events),
            "cases" => Ok(Self::Cases),
            "patterns" => Ok(Self::Patterns),
            _ => Err(format!("unknown category: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_display_lowercase() {
        assert_eq!(Category::Profile.to_string(), "profile");
        assert_eq!(Category::Preferences.to_string(), "preferences");
        assert_eq!(Category::Entities.to_string(), "entities");
        assert_eq!(Category::Events.to_string(), "events");
        assert_eq!(Category::Cases.to_string(), "cases");
        assert_eq!(Category::Patterns.to_string(), "patterns");
    }

    #[test]
    fn category_from_str() {
        assert_eq!("profile".parse::<Category>().unwrap(), Category::Profile);
        assert_eq!("EVENTS".parse::<Category>().unwrap(), Category::Events);
        assert!("unknown".parse::<Category>().is_err());
    }

    #[test]
    fn category_serde_roundtrip() {
        let c = Category::Cases;
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"cases\"");
        let parsed: Category = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn is_always_merge_only_profile() {
        assert!(Category::Profile.is_always_merge());
        assert!(!Category::Preferences.is_always_merge());
        assert!(!Category::Entities.is_always_merge());
        assert!(!Category::Events.is_always_merge());
        assert!(!Category::Cases.is_always_merge());
        assert!(!Category::Patterns.is_always_merge());
    }

    #[test]
    fn is_append_only_events_and_cases() {
        assert!(!Category::Profile.is_append_only());
        assert!(!Category::Preferences.is_append_only());
        assert!(!Category::Entities.is_append_only());
        assert!(Category::Events.is_append_only());
        assert!(Category::Cases.is_append_only());
        assert!(!Category::Patterns.is_append_only());
    }

    #[test]
    fn is_temporal_versioned_preferences_and_entities() {
        assert!(!Category::Profile.is_temporal_versioned());
        assert!(Category::Preferences.is_temporal_versioned());
        assert!(Category::Entities.is_temporal_versioned());
        assert!(!Category::Events.is_temporal_versioned());
        assert!(!Category::Cases.is_temporal_versioned());
        assert!(!Category::Patterns.is_temporal_versioned());
    }

    #[test]
    fn is_merge_supported_prefs_entities_patterns() {
        assert!(!Category::Profile.is_merge_supported());
        assert!(Category::Preferences.is_merge_supported());
        assert!(Category::Entities.is_merge_supported());
        assert!(!Category::Events.is_merge_supported());
        assert!(!Category::Cases.is_merge_supported());
        assert!(Category::Patterns.is_merge_supported());
    }
}
