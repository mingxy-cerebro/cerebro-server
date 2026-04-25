use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tracing::{info, warn};

use crate::api::event_bus::{EventBus, ServerEvent};
use crate::cluster::assigner::ClusterAssigner;
use crate::cluster::cluster_store::ClusterStore;
use crate::cluster::manager::ClusterManager;
use crate::domain::error::OmemError;
use crate::store::lancedb::LanceStore;

const MEMORY_THROTTLE: Duration = Duration::from_millis(200);
const BATCH_COOLDOWN: Duration = Duration::from_secs(2);

pub struct BackgroundClusterer {
    store: Arc<LanceStore>,
    cluster_assigner: Arc<ClusterAssigner>,
    cluster_manager: Arc<ClusterManager>,
    event_bus: Option<Arc<EventBus>>,
    tenant_id: String,
}

impl BackgroundClusterer {
    pub async fn new(
        store: Arc<LanceStore>,
        cluster_store: Arc<ClusterStore>,
        embed: Arc<dyn crate::embed::EmbedService>,
        llm: Option<Arc<dyn crate::llm::LlmService>>,
    ) -> Result<Self, OmemError> {
        let cluster_manager = Arc::new(ClusterManager::new(cluster_store.clone(), llm.clone()));
        let mut assigner = ClusterAssigner::new(cluster_store, embed);
        if let Some(llm) = llm {
            assigner = assigner.with_llm(llm);
        }
        Ok(Self {
            store,
            cluster_assigner: Arc::new(assigner),
            cluster_manager,
            event_bus: None,
            tenant_id: String::new(),
        })
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>, tenant_id: String) -> Self {
        self.event_bus = Some(bus);
        self.tenant_id = tenant_id;
        self
    }

    pub fn set_event_bus(&mut self, bus: Arc<EventBus>, tenant_id: String) {
        self.event_bus = Some(bus);
        self.tenant_id = tenant_id;
    }

    fn emit(&self, event_type: &str, data: serde_json::Value) {
        if let Some(bus) = &self.event_bus {
            bus.publish(ServerEvent {
                event_type: event_type.to_string(),
                tenant_id: self.tenant_id.clone(),
                data: Some(data),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
    }

    pub async fn cluster_all_unassigned(&self, batch_size: usize) -> Result<ClusterStats, OmemError> {
        info!("Starting background clustering of unassigned memories");

        self.emit("cluster.stage", serde_json::json!({
            "stage": "loading",
            "message": "Loading unassigned memories..."
        }));

        let memories = self.store.list_all_active().await?;
        let unassigned: Vec<_> = memories
            .into_iter()
            .filter(|m| m.cluster_id.is_none() && m.state != crate::domain::types::MemoryState::Deleted)
            .collect();

        let total = unassigned.len();
        info!(total, "Found unassigned memories to cluster");

        if total == 0 {
            self.emit("cluster.stage", serde_json::json!({
                "stage": "done",
                "message": "No unassigned memories found"
            }));
            return Ok(ClusterStats {
                processed: 0,
                assigned_to_existing: 0,
                created_new_clusters: 0,
                errors: 0,
            });
        }

        self.emit("cluster.started", serde_json::json!({
            "total": total,
            "batch_size": batch_size,
            "message": format!("Starting clustering of {} memories", total)
        }));

        let mut stats = ClusterStats {
            processed: 0,
            assigned_to_existing: 0,
            created_new_clusters: 0,
            errors: 0,
        };

        for (chunk_idx, chunk) in unassigned.chunks(batch_size).enumerate() {
            self.emit("cluster.batch_start", serde_json::json!({
                "batch": chunk_idx + 1,
                "batch_size": chunk.len(),
                "processed": stats.processed,
                "total": total,
            }));

            for memory in chunk {
                let content_preview: String = memory.content.chars().take(60).collect();
                let stage = "assigning";

                self.emit("cluster.memory_progress", serde_json::json!({
                    "memory_id": memory.id,
                    "content_preview": content_preview,
                    "stage": stage,
                    "processed": stats.processed,
                    "total": total,
                    "pct": if total > 0 { (stats.processed as f64 / total as f64 * 100.0).round() as u32 } else { 0 },
                }));

                match self.cluster_assigner.assign(memory).await {
                    Ok(result) => {
                        stats.processed += 1;
                        match result.action {
                            crate::cluster::assigner::AssignAction::AutoAssign => {
                                if let Some(cluster_id) = result.cluster_id {
                                    self.emit("cluster.memory_progress", serde_json::json!({
                                        "memory_id": memory.id,
                                        "stage": "linking",
                                        "action": "assign_existing",
                                        "cluster_id": cluster_id,
                                    }));
                                    if let Err(e) = self.cluster_manager.assign_to_cluster(
                                        &memory.id,
                                        &cluster_id,
                                        self.store.clone(),
                                    ).await {
                                        warn!(memory_id = %memory.id, error = %e, "Failed to assign memory to existing cluster");
                                        stats.errors += 1;
                                    } else {
                                        stats.assigned_to_existing += 1;
                                    }
                                }
                            }
                            crate::cluster::assigner::AssignAction::CreateNew => {
                                self.emit("cluster.memory_progress", serde_json::json!({
                                    "memory_id": memory.id,
                                    "stage": "creating_cluster",
                                    "action": "create_new",
                                }));
                                match self.store.get_vector_by_id(&memory.id).await {
                                    Ok(Some(vector)) => {
                                        match self.cluster_manager.create_cluster(
                                            memory,
                                            &vector,
                                        ).await {
                                            Ok(cluster) => {
                                                if let Err(e) = self.cluster_manager.assign_to_cluster(
                                                    &memory.id,
                                                    &cluster.id,
                                                    self.store.clone(),
                                                ).await {
                                                    warn!(memory_id = %memory.id, error = %e, "Failed to link memory to new cluster");
                                                    stats.errors += 1;
                                                } else {
                                                    stats.created_new_clusters += 1;
                                                }
                                            }
                                            Err(e) => {
                                                warn!(memory_id = %memory.id, error = %e, "Failed to create new cluster");
                                                stats.errors += 1;
                                            }
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
                sleep(MEMORY_THROTTLE).await;
            }

            info!(
                processed = stats.processed,
                total,
                "Clustering progress: {}/{} memories processed",
                stats.processed,
                total
            );

            self.emit("cluster.batch_done", serde_json::json!({
                "batch": chunk_idx + 1,
                "processed": stats.processed,
                "total": total,
                "assigned": stats.assigned_to_existing,
                "created_new": stats.created_new_clusters,
                "errors": stats.errors,
            }));

            sleep(BATCH_COOLDOWN).await;
        }

        info!(
            processed = stats.processed,
            assigned_to_existing = stats.assigned_to_existing,
            created_new_clusters = stats.created_new_clusters,
            errors = stats.errors,
            "Background clustering completed"
        );

        self.emit("cluster.complete", serde_json::json!({
            "processed": stats.processed,
            "assigned": stats.assigned_to_existing,
            "created_new": stats.created_new_clusters,
            "errors": stats.errors,
            "total": total,
        }));

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
