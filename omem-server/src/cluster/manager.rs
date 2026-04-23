use std::sync::Arc;
use tracing::{debug, info};

use crate::cluster::cluster_store::ClusterStore;
use crate::domain::cluster::MemoryCluster;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;

pub struct ClusterManager {
    cluster_store: Arc<ClusterStore>,
}

impl ClusterManager {
    pub fn new(cluster_store: Arc<ClusterStore>) -> Self {
        Self { cluster_store }
    }

    pub async fn create_cluster(
        &self,
        memory: &Memory,
        anchor_vector: &[f32],
    ) -> Result<MemoryCluster, OmemError> {
        let cluster = MemoryCluster::new(
            memory.tenant_id.clone(),
            memory.space_id.clone(),
            &memory.content[..memory.content.len().min(50)],
            &memory.l0_abstract,
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
        lance_store: &crate::store::LanceStore,
    ) -> Result<(), OmemError> {
        lance_store
            .update_memory_cluster_id(memory_id, Some(cluster_id), false)
            .await?;
        self.cluster_store.increment_member_count(cluster_id).await?;

        debug!(memory_id, cluster_id, "assigned memory to cluster");
        Ok(())
    }

    pub async fn update_cluster_summary(
        &self,
        cluster_id: &str,
        new_summary: &str,
    ) -> Result<(), OmemError> {
        self.cluster_store.update_summary(cluster_id, new_summary).await?;
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
