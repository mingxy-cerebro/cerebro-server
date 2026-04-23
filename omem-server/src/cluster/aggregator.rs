use std::sync::Arc;

use serde::Serialize;

use crate::cluster::cluster_store::ClusterStore;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;

pub struct ClusterAggregator {
    cluster_store: Arc<ClusterStore>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusteredResult {
    pub cluster_summaries: Vec<ClusterSummary>,
    pub standalone_memories: Vec<Memory>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterSummary {
    pub cluster_id: String,
    pub title: String,
    pub summary: String,
    pub member_count: u32,
    pub relevance_score: f32,
    pub key_memories: Vec<Memory>,
}

impl ClusterAggregator {
    pub fn new(cluster_store: Arc<ClusterStore>) -> Self {
        Self { cluster_store }
    }

    pub async fn aggregate(
        &self,
        memories: Vec<Memory>,
    ) -> Result<ClusteredResult, OmemError> {
        let mut cluster_map: std::collections::HashMap<String, Vec<Memory>> = std::collections::HashMap::new();
        let mut standalone: Vec<Memory> = Vec::new();

        for mem in memories {
            if let Some(ref cid) = mem.cluster_id {
                cluster_map.entry(cid.clone()).or_default().push(mem);
            } else {
                standalone.push(mem);
            }
        }

        let mut cluster_summaries = Vec::new();
        for (cid, members) in cluster_map {
            if let Some(cluster) = self.cluster_store.get_by_id(&cid).await? {
                let total_importance: f32 = members.iter().map(|m| m.importance).sum();
                let avg_importance = if members.is_empty() { 0.0 } else { total_importance / members.len() as f32 };
                let max_importance = members.iter().map(|m| m.importance).fold(0.0, f32::max);
                let weighted_score = avg_importance * 0.6 + max_importance * 0.4;
                cluster_summaries.push(ClusterSummary {
                    cluster_id: cid,
                    title: cluster.title,
                    summary: cluster.summary,
                    member_count: cluster.member_count,
                    relevance_score: weighted_score,
                    key_memories: members.into_iter().take(3).collect(),
                });
            }
        }

        cluster_summaries.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());

        Ok(ClusteredResult {
            cluster_summaries,
            standalone_memories: standalone,
        })
    }
}