use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::api::event_bus::{EventBus, ServerEvent};
use crate::api::scheduler_control::SharedSchedulerControl;
use crate::cluster::cluster_store::ClusterStore;
use crate::cluster::kmeans;
use crate::cluster::manager::ClusterManager;
use crate::domain::error::OmemError;
use crate::store::lancedb::LanceStore;

pub struct BackgroundClusterer {
    store: Arc<LanceStore>,
    cluster_manager: Arc<ClusterManager>,
    event_bus: Option<Arc<EventBus>>,
    scheduler_control: Option<SharedSchedulerControl>,
    tenant_id: String,
}

impl BackgroundClusterer {
    pub async fn new(
        store: Arc<LanceStore>,
        cluster_store: Arc<ClusterStore>,
        _embed: Arc<dyn crate::embed::EmbedService>,
        llm: Option<Arc<dyn crate::llm::LlmService>>,
    ) -> Result<Self, OmemError> {
        let cluster_manager = Arc::new(ClusterManager::new(cluster_store, llm));
        Ok(Self {
            store,
            cluster_manager,
            event_bus: None,
            scheduler_control: None,
            tenant_id: String::new(),
        })
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>, tenant_id: String) -> Self {
        self.event_bus = Some(bus);
        self.tenant_id = tenant_id;
        self
    }

    pub fn with_scheduler_control(mut self, ctrl: SharedSchedulerControl) -> Self {
        self.scheduler_control = Some(ctrl);
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

    pub async fn cluster_all_unassigned(&self, _batch_size: usize) -> Result<ClusterStats, OmemError> {
        self.cluster_global_kmeans().await
    }

    pub async fn cluster_global_kmeans(&self) -> Result<ClusterStats, OmemError> {
        info!("Starting global K-Means clustering");

        self.emit("cluster.stage", serde_json::json!({
            "stage": "loading",
            "message": "Loading all memory vectors..."
        }));

        let vectors_with_ids = self.store.get_all_vectors().await?;
        let memories = self.store.list_all_active().await?;

        if vectors_with_ids.is_empty() || memories.is_empty() {
            self.emit("cluster.stage", serde_json::json!({
                "stage": "done",
                "message": "No memories to cluster"
            }));
            return Ok(ClusterStats {
                processed: 0,
                assigned_to_existing: 0,
                created_new_clusters: 0,
                errors: 0,
            });
        }

        let tenant_id = memories[0].tenant_id.clone();
        let memory_map: HashMap<String, crate::domain::memory::Memory> = memories
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect();

        let mut ids: Vec<String> = Vec::with_capacity(vectors_with_ids.len());
        let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(vectors_with_ids.len());
        for (id, vec) in vectors_with_ids {
            if memory_map.contains_key(&id) {
                ids.push(id);
                vectors.push(vec);
            }
        }

        let total = ids.len();
        info!(total, "Found memories with vectors to cluster");

        self.emit("cluster.started", serde_json::json!({
            "total": total,
            "message": format!("Starting global K-Means clustering of {} memories", total)
        }));

        let (result, vectors) = tokio::task::spawn_blocking(move || {
            let result = kmeans::kmeans(&vectors, 50, 100);
            (result, vectors)
        })
        .await
        .map_err(|e| OmemError::Internal(format!("kmeans spawn_blocking error: {e}")))?;

        self.store.clear_all_cluster_ids().await?;
        self.cluster_manager.cluster_store().delete_all_clusters_by_tenant(&tenant_id).await?;

        let mut stats = ClusterStats {
            processed: 0,
            assigned_to_existing: 0,
            created_new_clusters: 0,
            errors: 0,
        };

        let mut cluster_members: HashMap<usize, Vec<usize>> = HashMap::new();
        for (idx, &label) in result.labels.iter().enumerate() {
            cluster_members.entry(label).or_default().push(idx);
        }

        for (_label, member_indices) in cluster_members {
            let anchor_idx = match member_indices.first() {
                Some(&i) => i,
                None => continue,
            };
            let anchor_id = &ids[anchor_idx];
            let anchor_memory = match memory_map.get(anchor_id) {
                Some(m) => m,
                None => {
                    stats.errors += 1;
                    continue;
                }
            };
            let anchor_vector = &vectors[anchor_idx];

            let cluster = match self.cluster_manager.create_cluster(anchor_memory, anchor_vector).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(memory_id = %anchor_id, error = %e, "Failed to create cluster");
                    stats.errors += 1;
                    continue;
                }
            };

            stats.created_new_clusters += 1;

            for &member_idx in &member_indices {
                let member_id = &ids[member_idx];
                if member_id == anchor_id {
                    stats.processed += 1;
                    continue;
                }
                if let Err(e) = self.cluster_manager.assign_to_cluster(
                    member_id,
                    &cluster.id,
                    self.store.clone(),
                ).await {
                    warn!(memory_id = %member_id, error = %e, "Failed to assign memory to cluster");
                    stats.errors += 1;
                } else {
                    stats.assigned_to_existing += 1;
                    stats.processed += 1;
                }
            }
        }

        info!(
            processed = stats.processed,
            created_new_clusters = stats.created_new_clusters,
            assigned_to_existing = stats.assigned_to_existing,
            errors = stats.errors,
            "Global K-Means clustering completed"
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
