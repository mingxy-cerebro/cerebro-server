use std::sync::Arc;
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::cluster::cluster_store::ClusterStore;
use crate::domain::cluster::MemoryCluster;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::ingest::prompts;
use crate::llm::{complete_json, LlmService};

#[derive(Deserialize)]
struct ClusterSummaryResponse {
    title: String,
    summary: String,
}

pub struct ClusterManager {
    cluster_store: Arc<ClusterStore>,
    llm: Option<Arc<dyn LlmService>>,
}

impl ClusterManager {
    pub fn new(cluster_store: Arc<ClusterStore>, llm: Option<Arc<dyn LlmService>>) -> Self {
        Self {
            cluster_store,
            llm,
        }
    }

    pub fn cluster_store(&self) -> &Arc<ClusterStore> {
        &self.cluster_store
    }

    pub fn llm(&self) -> Option<&Arc<dyn LlmService>> {
        self.llm.as_ref()
    }

    fn get_dedup_threshold() -> f32 {
        std::env::var("OMEM_CLUSTER_DEDUP_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.85)
    }

    pub async fn create_cluster(
        &self,
        memory: &Memory,
        anchor_vector: &[f32],
    ) -> Result<MemoryCluster, OmemError> {
        let dedup_threshold = Self::get_dedup_threshold();

        let candidates = self
            .cluster_store
            .search_by_vector(anchor_vector, 3, Some(&memory.space_id))
            .await
            .unwrap_or_default();

        if let Some((existing, score)) = candidates.first() {
            if *score >= dedup_threshold {
                info!(
                    cluster_id = %existing.id,
                    memory_id = %memory.id,
                    similarity = score,
                    "reusing existing cluster (vector dedup)"
                );
                return Ok(existing.clone());
            }
        }

        let (title, summary) = if let Some(ref llm) = self.llm {
            let (system, user) =
                prompts::build_cluster_initial_summary_prompt(&memory.content, &memory.l0_abstract);
            match complete_json::<ClusterSummaryResponse>(llm.as_ref(), &system, &user).await {
                Ok(resp) => (resp.title, resp.summary),
                Err(e) => {
                    warn!(
                        error = %e,
                        memory_id = %memory.id,
                        "failed to generate cluster title/summary via LLM, using fallback"
                    );
                    let fallback_title: String = memory.content.chars().take(50).collect();
                    (fallback_title, memory.l0_abstract.clone())
                }
            }
        } else {
            let fallback_title: String = memory.content.chars().take(50).collect();
            (fallback_title, memory.l0_abstract.clone())
        };

        let existing_clusters = self
            .cluster_store
            .list_clusters_by_tenant(&memory.tenant_id, 100, 0)
            .await
            .unwrap_or_default();

        for existing in &existing_clusters {
            if existing.title == title {
                info!(
                    cluster_id = %existing.id,
                    memory_id = %memory.id,
                    title = %title,
                    "reusing existing cluster (title dedup)"
                );
                return Ok(existing.clone());
            }
        }

        let cluster = MemoryCluster::new(
            memory.tenant_id.clone(),
            memory.space_id.clone(),
            title,
            summary,
            memory.category.clone(),
            memory.id.clone(),
        );

        self.cluster_store.create(&cluster, anchor_vector).await?;

        info!(cluster_id = %cluster.id, memory_id = %memory.id, "created new cluster");
        Ok(cluster)
    }

    pub async fn assign_to_cluster(
        &self,
        memory_id: &str,
        cluster_id: &str,
        lance_store: Arc<crate::store::LanceStore>,
    ) -> Result<(), OmemError> {
        lance_store
            .update_memory_cluster_id(memory_id, Some(cluster_id), false)
            .await?;
        let new_count = self.cluster_store.increment_member_count(cluster_id).await?;

        debug!(memory_id, cluster_id, "assigned memory to cluster");

        Ok(())
    }

    pub async fn regenerate_summary(
        cluster_store: &ClusterStore,
        lance_store: &crate::store::LanceStore,
        llm: &dyn LlmService,
        cluster_id: &str,
    ) -> Result<(), OmemError> {
        let cluster = cluster_store
            .get_by_id(cluster_id)
            .await?
            .ok_or_else(|| OmemError::NotFound(format!("cluster {cluster_id} not found")))?;

        let members = lance_store.list_by_cluster_id(cluster_id).await?;
        if members.is_empty() {
            return Ok(());
        }

        let member_contents: Vec<String> = members.iter().map(|m| m.content.clone()).collect();
        let (system, user) =
            prompts::build_cluster_summary_prompt(&cluster.title, &cluster.summary, &member_contents);
        let resp: ClusterSummaryResponse = complete_json(llm, &system, &user).await?;

        cluster_store.update_summary(cluster_id, &resp.summary).await?;
        cluster_store.update_title(cluster_id, &resp.title).await?;

        info!(
            cluster_id,
            title = %resp.title,
            "regenerated cluster title and summary"
        );
        Ok(())
    }

    pub async fn update_cluster_summary(
        &self,
        cluster_id: &str,
        new_summary: &str,
    ) -> Result<(), OmemError> {
        self.cluster_store
            .update_summary(cluster_id, new_summary)
            .await?;
        Ok(())
    }

    pub async fn on_memory_removed(
        &self,
        memory: &Memory,
    ) -> Result<(), OmemError> {
        if let Some(ref cluster_id) = memory.cluster_id {
            self.cluster_store.decrement_member_count(cluster_id).await?;
            let cluster = self.cluster_store.get_by_id(cluster_id).await?;
            if let Some(c) = cluster {
                if c.member_count == 0 {
                    info!(cluster_id, "cluster became empty after memory removal");
                }
            }
        }
        Ok(())
    }
}
