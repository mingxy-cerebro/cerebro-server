use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::category::Category;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryCluster {
    pub id: String,
    pub tenant_id: String,
    pub space_id: String,
    pub title: String,
    pub summary: String,
    pub category: Category,
    pub member_count: u32,
    pub importance: f32,
    pub keywords: Vec<String>,
    pub tags: Vec<String>,
    pub anchor_memory_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: Option<String>,
}

impl MemoryCluster {
    pub fn new(
        tenant_id: impl Into<String>,
        space_id: impl Into<String>,
        title: impl Into<String>,
        summary: impl Into<String>,
        category: Category,
        anchor_memory_id: impl Into<String>,
    ) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant_id.into(),
            space_id: space_id.into(),
            title: title.into(),
            summary: summary.into(),
            category,
            member_count: 1,
            importance: 0.5,
            keywords: Vec::new(),
            tags: Vec::new(),
            anchor_memory_id: anchor_memory_id.into(),
            created_at: now.clone(),
            updated_at: now,
            last_accessed_at: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClusterMembership {
    pub memory_id: String,
    pub cluster_id: String,
    pub contribution: String,
    pub added_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ClusteringJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClusteringJob {
    pub id: String,
    pub tenant_id: String,
    pub space_id: String,
    pub status: ClusteringJobStatus,
    pub total_memories: u64,
    pub processed_memories: u64,
    pub assigned_to_existing: u64,
    pub created_new_clusters: u64,
    pub errors: u64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
}

impl ClusteringJob {
    pub fn new(tenant_id: impl Into<String>, space_id: impl Into<String>, total_memories: u64) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant_id.into(),
            space_id: space_id.into(),
            status: ClusteringJobStatus::Pending,
            total_memories,
            processed_memories: 0,
            assigned_to_existing: 0,
            created_new_clusters: 0,
            errors: 0,
            started_at: None,
            completed_at: None,
            error_message: None,
            created_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::Category;

    #[test]
    fn cluster_new_defaults() {
        let cluster = MemoryCluster::new(
            "t-001",
            "space-001",
            "WSL2环境配置",
            "WSL2相关的环境配置问题汇总",
            Category::Preferences,
            "mem-001",
        );

        assert!(!cluster.id.is_empty());
        assert_eq!(cluster.title, "WSL2环境配置");
        assert_eq!(cluster.member_count, 1);
        assert_eq!(cluster.category, Category::Preferences);
        assert!(cluster.keywords.is_empty());
    }
}
