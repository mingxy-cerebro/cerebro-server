use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::category::Category;
use crate::domain::relation::MemoryRelation;
use crate::domain::space::Provenance;
use crate::domain::types::{MemoryState, MemoryType, Tier};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub l0_abstract: String,
    pub l1_overview: String,
    pub l2_content: String,
    pub category: Category,
    pub memory_type: MemoryType,
    pub state: MemoryState,
    pub tier: Tier,
    pub importance: f32,
    pub confidence: f32,
    pub access_count: u32,
    pub tags: Vec<String>,
    pub scope: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub tenant_id: String,
    pub source: Option<String>,
    pub relations: Vec<MemoryRelation>,
    pub superseded_by: Option<String>,
    pub invalidated_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: Option<String>,
    pub space_id: String,
    /// "private" | "global" | "shared:<group-id>"
    pub visibility: String,
    pub owner_agent_id: String,
    pub provenance: Option<Provenance>,
    #[serde(default)]
    pub version: Option<u64>,
    #[serde(default)]
    pub tier_history: Option<String>,
    #[serde(default)]
    pub cluster_id: Option<String>,
    #[serde(default)]
    pub is_cluster_anchor: bool,
}

impl Memory {
    pub fn new(
        content: impl Into<String>,
        category: Category,
        memory_type: MemoryType,
        tenant_id: impl Into<String>,
    ) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        let content = content.into();
        Self {
            id: Uuid::new_v4().to_string(),
            l0_abstract: String::new(),
            l1_overview: String::new(),
            l2_content: content.clone(),
            content,
            category,
            memory_type,
            state: MemoryState::Active,
            tier: Tier::Peripheral,
            importance: 0.5,
            confidence: 0.5,
            access_count: 0,
            tags: Vec::new(),
            scope: "global".to_string(),
            agent_id: None,
            session_id: None,
            tenant_id: tenant_id.into(),
            source: None,
            relations: Vec::new(),
            superseded_by: None,
            invalidated_at: None,
            created_at: now.clone(),
            updated_at: now,
            last_accessed_at: None,
            space_id: String::new(),
            visibility: "global".to_string(),
            owner_agent_id: String::new(),
            provenance: None,
            version: Some(1),
            tier_history: None,
            cluster_id: None,
            is_cluster_anchor: false,
        }
    }

    pub fn append_tier_change(&mut self, from: &str, to: &str, reason: &str) {
        let event = serde_json::json!({
            "from": from,
            "to": to,
            "reason": reason,
            "at": chrono::Utc::now().to_rfc3339(),
            "access_count": self.access_count,
        });
        let mut history: Vec<serde_json::Value> = match &self.tier_history {
            Some(h) if !h.is_empty() => serde_json::from_str(h).unwrap_or_default(),
            _ => Vec::new(),
        };
        history.push(event);
        self.tier_history = Some(serde_json::to_string(&history).unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::relation::RelationType;

    #[test]
    fn memory_new_defaults() {
        let mem = Memory::new(
            "user prefers dark mode",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );

        assert!(!mem.id.is_empty());
        assert_eq!(mem.content, "user prefers dark mode");
        assert_eq!(mem.l2_content, "user prefers dark mode");
        assert!(mem.l0_abstract.is_empty());
        assert!(mem.l1_overview.is_empty());
        assert_eq!(mem.category, Category::Preferences);
        assert_eq!(mem.memory_type, MemoryType::Insight);
        assert_eq!(mem.state, MemoryState::Active);
        assert_eq!(mem.tier, Tier::Peripheral);
        assert!((mem.importance - 0.5).abs() < f32::EPSILON);
        assert!((mem.confidence - 0.5).abs() < f32::EPSILON);
        assert_eq!(mem.access_count, 0);
        assert!(mem.tags.is_empty());
        assert_eq!(mem.scope, "global");
        assert!(mem.agent_id.is_none());
        assert!(mem.session_id.is_none());
        assert_eq!(mem.tenant_id, "t-001");
        assert!(mem.source.is_none());
        assert!(mem.relations.is_empty());
        assert!(mem.superseded_by.is_none());
        assert!(mem.invalidated_at.is_none());
        assert!(!mem.created_at.is_empty());
        assert_eq!(mem.created_at, mem.updated_at);
        assert!(mem.last_accessed_at.is_none());
        assert!(mem.space_id.is_empty());
        assert_eq!(mem.visibility, "global");
        assert!(mem.owner_agent_id.is_empty());
        assert!(mem.provenance.is_none());
        assert_eq!(mem.version, Some(1));
    }

    #[test]
    fn memory_serde_roundtrip() {
        let mut mem = Memory::new(
            "test content",
            Category::Events,
            MemoryType::Session,
            "t-002",
        );
        mem.tags = vec!["tag1".to_string(), "tag2".to_string()];
        mem.relations.push(MemoryRelation {
            relation_type: RelationType::Supports,
            target_id: "mem-old".to_string(),
            context_label: None,
        });

        let json = serde_json::to_string(&mem).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, mem.id);
        assert_eq!(parsed.content, "test content");
        assert_eq!(parsed.category, Category::Events);
        assert_eq!(parsed.tags.len(), 2);
        assert_eq!(parsed.relations.len(), 1);
        assert_eq!(parsed.relations[0].relation_type, RelationType::Supports);
    }

    #[test]
    fn memory_id_is_uuid_v4() {
        let mem = Memory::new("test", Category::Profile, MemoryType::Pinned, "t-001");
        let parsed = Uuid::parse_str(&mem.id);
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().get_version_num(), 4);
    }
}
