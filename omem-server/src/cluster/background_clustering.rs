use std::sync::Arc;
use tracing::{info, warn};

use crate::cluster::assigner::ClusterAssigner;
use crate::cluster::cluster_store::ClusterStore;
use crate::cluster::manager::ClusterManager;
use crate::domain::error::OmemError;
use crate::store::lancedb::LanceStore;

pub struct BackgroundClusterer {
    store: Arc<LanceStore>,
    cluster_assigner: Arc<ClusterAssigner>,
    cluster_manager: Arc<ClusterManager>,
}

impl BackgroundClusterer {
    pub async fn new(
        store: Arc<LanceStore>,
        cluster_store: Arc<ClusterStore>,
        embed: Arc<dyn crate::embed::EmbedService>,
        llm: Option<Arc<dyn crate::llm::LlmService>>,
    ) -> Result<Self, OmemError> {
        let cluster_manager = Arc::new(ClusterManager::new(cluster_store.clone()));
        let mut assigner = ClusterAssigner::new(cluster_store, embed);
        if let Some(llm) = llm {
            assigner = assigner.with_llm(llm);
        }
        Ok(Self {
            store,
            cluster_assigner: Arc::new(assigner),
            cluster_manager,
        })
    }

    pub async fn cluster_all_unassigned(&self, batch_size: usize) -> Result<ClusterStats, OmemError> {
        info!("Starting background clustering of unassigned memories");
        
        let memories = self.store.list_all_active().await?;
        let unassigned: Vec<_> = memories
            .into_iter()
            .filter(|m| m.cluster_id.is_none() && m.state != crate::domain::types::MemoryState::Deleted)
            .collect();
        
        let total = unassigned.len();
        info!(total, "Found unassigned memories to cluster");
        
        if total == 0 {
            return Ok(ClusterStats {
                processed: 0,
                assigned_to_existing: 0,
                created_new_clusters: 0,
                errors: 0,
            });
        }
        
        let mut stats = ClusterStats {
            processed: 0,
            assigned_to_existing: 0,
            created_new_clusters: 0,
            errors: 0,
        };
        
        for chunk in unassigned.chunks(batch_size) {
            for memory in chunk {
                match self.cluster_assigner.assign(memory).await {
                    Ok(result) => {
                        stats.processed += 1;
                        match result.action {
                            crate::cluster::assigner::AssignAction::AutoAssign => {
                                if let Some(cluster_id) = result.cluster_id {
                                    if let Err(e) = self.cluster_manager.assign_to_cluster(
                                        &memory.id,
                                        &cluster_id,
                                        &self.store,
                                    ).await {
                                        warn!(memory_id = %memory.id, error = %e, "Failed to assign memory to existing cluster");
                                        stats.errors += 1;
                                    } else {
                                        stats.assigned_to_existing += 1;
                                    }
                                }
                            }
                            crate::cluster::assigner::AssignAction::CreateNew => {
                                match self.store.get_vector_by_id(&memory.id).await {
                                    Ok(Some(vector)) => {
                                        if let Err(e) = self.cluster_manager.create_cluster(
                                            memory,
                                            &vector,
                                        ).await {
                                            warn!(memory_id = %memory.id, error = %e, "Failed to create new cluster");
                                            stats.errors += 1;
                                        } else {
                                            stats.created_new_clusters += 1;
                                        }
                                    }
                                    Ok(None) => {
                                        warn!(memory_id = %memory.id, "No vector found for memory, skipping cluster creation");
                                        stats.errors += 1;
                                    }
                                    Err(e) => {
                                        warn!(memory_id = %memory.id, error = %e, "Failed to get vector for memory");
                                        stats.errors += 1;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        stats.errors += 1;
                        warn!(memory_id = %memory.id, error = %e, "Failed to assign memory to cluster");
                    }
                }
            }
            
            info!(
                processed = stats.processed,
                total,
                "Clustering progress: {}/{} memories processed",
                stats.processed,
                total
            );
        }
        
        info!(
            processed = stats.processed,
            assigned_to_existing = stats.assigned_to_existing,
            created_new_clusters = stats.created_new_clusters,
            errors = stats.errors,
            "Background clustering completed"
        );
        
        Ok(stats)
    }
}

#[derive(Debug, Clone)]
pub struct ClusterStats {
    pub processed: usize,
    pub assigned_to_existing: usize,
    pub created_new_clusters: usize,
    pub errors: usize,
}