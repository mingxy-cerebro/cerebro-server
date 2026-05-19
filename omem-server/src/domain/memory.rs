use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::category::Category;
use crate::domain::relation::MemoryRelation;
use crate::domain::space::Provenance;
use crate::domain::types::{MemoryState, MemoryType, Tier};

/// Sanitize a project_path to prevent path traversal and SQL injection.
///
/// Rules:
/// - Max 512 characters
/// - No `..` (path traversal)
/// - No `'`, `;`, `--` (SQL injection vectors)
pub fn sanitize_project_path(path: &str) -> Result<String, String> {
    if path.len() > 512 {
        return Err("project_path too long (max 512 chars)".to_string());
    }
    if path.contains("..") {
        return Err("project_path contains path traversal".to_string());
    }
    if path.contains('\'') || path.contains(';') || path.contains("--") {
        return Err("project_path contains invalid characters".to_string());
    }
    Ok(path.to_string())
}

/// Lightweight digest of a memory for summary queries.
/// Avoids loading full content and vectors.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryDigest {
    pub id: String,
    pub title: String,
    pub category: Category,
    pub tags: Vec<String>,
    pub content_preview: String,
    pub updated_at: String,
}

/// Summary of session memories returned by fetch_session_* helpers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionMemorySummary {
    pub memories: Vec<MemoryDigest>,
    pub merged_summary: String,
    pub total_count: usize,
}

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
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub project_path: Option<String>,
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
            metadata: None,
            project_path: None,
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
            Category::new("preferences"),
            MemoryType::Insight,
            "t-001",
        );

        assert!(!mem.id.is_empty());
        assert_eq!(mem.content, "user prefers dark mode");
        assert_eq!(mem.l2_content, "user prefers dark mode");
        assert!(mem.l0_abstract.is_empty());
        assert!(mem.l1_overview.is_empty());
        assert_eq!(mem.category, Category::new("preferences"));
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
            Category::new("events"),
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
        assert_eq!(parsed.category, Category::new("events"));
        assert_eq!(parsed.tags.len(), 2);
        assert_eq!(parsed.relations.len(), 1);
        assert_eq!(parsed.relations[0].relation_type, RelationType::Supports);
    }

    #[test]
    fn memory_id_is_uuid_v4() {
        let mem = Memory::new("test", Category::new("profile"), MemoryType::Pinned, "t-001");
        let parsed = Uuid::parse_str(&mem.id);
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().get_version_num(), 4);
    }

    #[test]
    fn test_memory_default_project_path_none() {
        let mem = Memory::new(
            "test project_path",
            Category::new("events"),
            MemoryType::Session,
            "t-001",
        );
        assert_eq!(mem.project_path, None);
    }

    #[test]
    fn sanitize_valid_path_passes() {
        assert_eq!(
            sanitize_project_path("/mnt/d/dev/project"),
            Ok("/mnt/d/dev/project".to_string())
        );
    }

    #[test]
    fn sanitize_empty_path_passes() {
        assert_eq!(sanitize_project_path(""), Ok("".to_string()));
    }

    #[test]
    fn sanitize_rejects_path_traversal() {
        assert_eq!(
            sanitize_project_path("../etc/passwd"),
            Err("project_path contains path traversal".to_string())
        );
        assert_eq!(
            sanitize_project_path("foo/../../bar"),
            Err("project_path contains path traversal".to_string())
        );
    }

    #[test]
    fn sanitize_rejects_sql_injection() {
        assert_eq!(
            sanitize_project_path("'; DROP TABLE memories; --"),
            Err("project_path contains invalid characters".to_string())
        );
        assert_eq!(
            sanitize_project_path("path;rm -rf"),
            Err("project_path contains invalid characters".to_string())
        );
        assert_eq!(
            sanitize_project_path("path'OR'1"),
            Err("project_path contains invalid characters".to_string())
        );
    }

    #[test]
    fn sanitize_rejects_too_long() {
        let long_path = "a".repeat(513);
        assert_eq!(
            sanitize_project_path(&long_path),
            Err("project_path too long (max 512 chars)".to_string())
        );
        let max_path = "a".repeat(512);
        assert_eq!(sanitize_project_path(&max_path), Ok(max_path));
    }

    #[test]
    fn test_project_path_serde_roundtrip() {
        let mut mem = Memory::new(
            "serde test",
            Category::new("preferences"),
            MemoryType::Insight,
            "t-002",
        );
        mem.project_path = Some("/home/user/project".to_string());

        let json = serde_json::to_string(&mem).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.project_path, Some("/home/user/project".to_string()));

        mem.project_path = None;
        let json2 = serde_json::to_string(&mem).unwrap();
        let parsed2: Memory = serde_json::from_str(&json2).unwrap();
        assert_eq!(parsed2.project_path, None);
    }
}
