use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn, debug};

use crate::api::event_bus::{EventBus, ServerEvent};
use crate::api::scheduler_control::SharedSchedulerControl;
use crate::cluster::assigner::{AssignAction, ClusterAssigner};
use crate::cluster::cluster_store::ClusterStore;
use crate::cluster::kmeans;
use crate::cluster::manager::ClusterManager;
use crate::domain::cluster::MemoryCluster;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::ingest::prompts;
use crate::llm::complete_json;
use crate::store::lancedb::LanceStore;

use serde::Deserialize;

#[derive(Deserialize)]
struct ClusterSummaryResponse {
    title: String,
    summary: String,
}

struct NewClusterInfo {
    id: String,
    member_count: u32,
}

pub struct BackgroundClusterer {
    store: Arc<LanceStore>,
    cluster_store: Arc<ClusterStore>,
    embed: Arc<dyn crate::embed::EmbedService>,
    llm: Option<Arc<dyn crate::llm::LlmService>>,
    cluster_manager: Arc<ClusterManager>,
    event_bus: Option<Arc<EventBus>>,
    scheduler_control: Option<SharedSchedulerControl>,
    tenant_id: String,
    clustering_lock: Arc<tokio::sync::Mutex<()>>,
}

impl BackgroundClusterer {
    pub async fn new(
        store: Arc<LanceStore>,
        cluster_store: Arc<ClusterStore>,
        _embed: Arc<dyn crate::embed::EmbedService>,
        llm: Option<Arc<dyn crate::llm::LlmService>>,
    ) -> Result<Self, OmemError> {
        let cluster_manager = Arc::new(ClusterManager::new(cluster_store.clone(), llm.clone()));
        Ok(Self {
            store,
            cluster_store,
            embed: _embed,
            llm,
            cluster_manager,
            event_bus: None,
            scheduler_control: None,
            tenant_id: String::new(),
            clustering_lock: Arc::new(tokio::sync::Mutex::new(())),
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

    pub async fn cluster_all_unassigned(&self, batch_size: usize) -> Result<ClusterStats, OmemError> {
        let tenant = if self.tenant_id.is_empty() {
            "unknown"
        } else {
            &self.tenant_id
        };
        info!(batch_size, tenant, "cluster_all_unassigned: using safe incremental path (was destructive global k-means)");

        Self::run_incremental_clustering(
            self.store.clone(),
            self.cluster_store.clone(),
            self.embed.clone(),
            self.llm.clone(),
            Some(batch_size),
            tenant,
        )
        .await
    }

    pub async fn cluster_global_kmeans(&self) -> Result<ClusterStats, OmemError> {
        let lock = self.clustering_lock.try_lock();
        if lock.is_err() {
            info!("K-Means clustering already running, skipping duplicate");
            return Ok(ClusterStats {
                processed: 0,
                assigned_to_existing: 0,
                created_new_clusters: 0,
                errors: 0,
            });
        }
        let _guard = lock.map_err(|e| OmemError::Internal(format!("clustering lock: {e}")))?;

        info!("Starting global K-Means clustering");

        self.emit("cluster.stage", serde_json::json!({
            "stage": "loading",
            "message": "Loading all memory vectors..."
        }));

        let mut vectors_with_ids = self.store.get_all_vectors().await?;
        let memories = self.store.list_all_active(Some(5000)).await?;

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

        vectors_with_ids.sort_by(|a, b| a.0.cmp(&b.0));

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

        let existing = self.cluster_manager.cluster_store().list_clusters_by_tenant(&tenant_id, 1000, 0).await?;
        let existing_ids: Vec<String> = existing.iter().map(|c| c.id.clone()).collect();
        self.cluster_manager.cluster_store().batch_delete_clusters_by_ids(&existing_ids).await?;

        let mut cluster_members: HashMap<usize, Vec<usize>> = HashMap::new();
        for (idx, &label) in result.labels.iter().enumerate() {
            cluster_members.entry(label).or_default().push(idx);
        }

        let mut new_clusters: Vec<(MemoryCluster, Vec<f32>)> = Vec::new();
        let mut new_cluster_infos: Vec<NewClusterInfo> = Vec::new();
        let mut all_assignments: Vec<(String, Option<String>, bool)> = Vec::new();
        let mut stats = ClusterStats {
            processed: 0,
            assigned_to_existing: 0,
            created_new_clusters: 0,
            errors: 0,
        };

        for (_label, member_indices) in cluster_members {
            let anchor_idx = match member_indices.first() {
                Some(&i) => i,
                None => continue,
            };
            let anchor_id = &ids[anchor_idx];
            let anchor_memory = match memory_map.get(anchor_id) {
                Some(m) => m,
                None => {
                    stats.errors += member_indices.len();
                    continue;
                }
            };
            let anchor_vector = &vectors[anchor_idx];

            let (title, summary) = if let Some(llm) = self.cluster_manager.llm() {
                let (system, user) =
                    prompts::build_cluster_initial_summary_prompt(&anchor_memory.content, &anchor_memory.l0_abstract);
                match complete_json::<ClusterSummaryResponse>(llm.as_ref(), &system, &user).await {
                    Ok(resp) => (resp.title, resp.summary),
                    Err(e) => {
                        warn!(error = %e, memory_id = %anchor_id, "LLM summary failed, using fallback");
                        let fallback_title: String = anchor_memory.content.chars().take(50).collect();
                        (fallback_title, anchor_memory.l0_abstract.clone())
                    }
                }
            } else {
                let fallback_title: String = anchor_memory.content.chars().take(50).collect();
                (fallback_title, anchor_memory.l0_abstract.clone())
            };

            let mut cluster = MemoryCluster::new(
                anchor_memory.tenant_id.clone(),
                anchor_memory.space_id.clone(),
                title,
                summary,
                anchor_memory.category.clone(),
                anchor_memory.id.clone(),
            );
            cluster.member_count = member_indices.len() as u32;
            cluster.tags = super::manager::infer_cluster_tags(&anchor_memory.content, &anchor_memory.tags);

            let cluster_id = cluster.id.clone();
            new_cluster_infos.push(NewClusterInfo {
                id: cluster_id.clone(),
                member_count: member_indices.len() as u32,
            });

            for (i, &member_idx) in member_indices.iter().enumerate() {
                let is_anchor = i == 0;
                all_assignments.push((ids[member_idx].clone(), Some(cluster_id.clone()), is_anchor));
            }

            new_clusters.push((cluster, anchor_vector.clone()));
            stats.created_new_clusters += 1;
            stats.processed += member_indices.len();

            self.emit("cluster.memory_progress", serde_json::json!({
                "memory_id": anchor_id,
                "content_preview": anchor_memory.content.chars().take(40).collect::<String>(),
                "stage": "creating_cluster",
                "action": "create_new",
                "cluster_id": cluster_id,
                "processed": stats.processed,
                "total": total,
                "pct": (stats.processed as f64 / total as f64 * 100.0).round() as u32,
            }));
        }

        if !new_clusters.is_empty() {
            self.cluster_manager.cluster_store().batch_create_clusters(&new_clusters).await?;
        }

        if !all_assignments.is_empty() {
            self.store.batch_update_cluster_ids(&all_assignments).await?;
        }

        let member_counts: Vec<(&str, u32)> = new_cluster_infos.iter()
            .map(|c| (c.id.as_str(), c.member_count))
            .collect();
        if !member_counts.is_empty() {
            self.cluster_manager.cluster_store().batch_set_member_counts(&member_counts).await?;
        }

        stats.assigned_to_existing = total.saturating_sub(stats.errors + stats.created_new_clusters);

        let created_cluster_ids: Vec<String> = new_cluster_infos.iter().map(|c| c.id.clone()).collect();

        self.emit("cluster.stage", serde_json::json!({
            "stage": "summarizing",
            "message": format!("Generating summaries for {} clusters...", created_cluster_ids.len())
        }));

        for cluster_id in &created_cluster_ids {
            if let Some(llm) = self.cluster_manager.llm() {
                if let Err(e) = ClusterManager::regenerate_summary(
                    self.cluster_manager.cluster_store(),
                    &self.store,
                    llm.as_ref(),
                    cluster_id,
                ).await {
                    warn!(cluster_id, error = %e, "Failed to regenerate cluster summary");
                }
            }
        }

        info!(
            processed = stats.processed,
            created_new_clusters = stats.created_new_clusters,
            errors = stats.errors,
            "Global K-Means clustering completed (batch mode)"
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

    /// Incremental clustering: processes only memories with cluster_id == None.
    /// Non-destructive — existing clusters are never deleted or rebuilt.
    pub async fn run_incremental_clustering(
        store: Arc<LanceStore>,
        cluster_store: Arc<ClusterStore>,
        embed: Arc<dyn crate::embed::EmbedService>,
        llm: Option<Arc<dyn crate::llm::LlmService>>,
        batch_size: Option<usize>,
        tenant_id: &str,
    ) -> Result<ClusterStats, OmemError> {
        let limit = batch_size.unwrap_or(50).min(50);

        info!(batch_size = limit, tenant_id, "Starting incremental clustering");

        let cluster_manager = Arc::new(ClusterManager::new(cluster_store.clone(), llm.clone()));

        let mut assigner = ClusterAssigner::new(cluster_store, embed);
        if let Some(llm_svc) = llm {
            assigner = assigner.with_llm(llm_svc);
        }
        let assigner = assigner.with_lance_store(Some(store.clone()));

        let mut stats = ClusterStats {
            processed: 0,
            assigned_to_existing: 0,
            created_new_clusters: 0,
            errors: 0,
        };

        let mut offset = 0usize;
        let fetch_size = limit * 3;

        loop {
            let memories = match store.list(fetch_size, offset).await {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, offset, "incremental_clustering: failed to list memories");
                    break;
                }
            };

            if memories.is_empty() {
                break;
            }

            let unassigned: Vec<Memory> = memories
                .into_iter()
                .filter(|m| m.cluster_id.is_none() && m.state == crate::domain::types::MemoryState::Active)
                .collect();

            if unassigned.is_empty() {
                offset += fetch_size;
                continue;
            }

            let mut session_groups: HashMap<String, Vec<Memory>> = HashMap::new();
            let mut no_session: Vec<Memory> = Vec::new();

            for mem in unassigned {
                match &mem.session_id {
                    Some(sid) if !sid.is_empty() => {
                        session_groups.entry(sid.clone()).or_default().push(mem);
                    }
                    _ => {
                        no_session.push(mem);
                    }
                }
            }

            for (session_id, group) in &session_groups {
                if stats.processed >= limit {
                    break;
                }
                debug!(session_id = %session_id, count = group.len(), "incremental: session group");
                for memory in group {
                    if stats.processed >= limit {
                        break;
                    }
                    Self::process_single_memory(
                        &assigner,
                        &cluster_manager,
                        &store,
                        memory,
                        &mut stats,
                    ).await;
                }
                tokio::task::yield_now().await;
            }

            for memory in &no_session {
                if stats.processed >= limit {
                    break;
                }
                Self::process_single_memory(
                    &assigner,
                    &cluster_manager,
                    &store,
                    memory,
                    &mut stats,
                ).await;
                tokio::task::yield_now().await;
            }

            drop(session_groups);
            drop(no_session);

            if stats.processed >= limit {
                break;
            }

            offset += fetch_size;
        }

        info!(
            processed = stats.processed,
            assigned_to_existing = stats.assigned_to_existing,
            created_new_clusters = stats.created_new_clusters,
            errors = stats.errors,
            tenant_id,
            "Incremental clustering completed"
        );

        Ok(stats)
    }

    async fn process_single_memory(
        assigner: &ClusterAssigner,
        cluster_manager: &ClusterManager,
        store: &Arc<LanceStore>,
        memory: &Memory,
        stats: &mut ClusterStats,
    ) {
        match assigner.assign(memory).await {
            Ok(result) => {
                match result.action {
                    AssignAction::AutoAssign | AssignAction::LlmJudge => {
                        if let Some(ref cluster_id) = result.cluster_id {
                            match cluster_manager.assign_to_cluster(
                                &memory.id,
                                cluster_id,
                                (*store).clone(),
                            ).await {
                                Ok(()) => {
                                    debug!(memory_id = %memory.id, cluster_id, confidence = result.confidence, "incremental: assigned");
                                    stats.assigned_to_existing += 1;
                                    stats.processed += 1;
                                }
                                Err(e) => {
                                    warn!(memory_id = %memory.id, error = %e, "incremental: assign failed");
                                    stats.errors += 1;
                                }
                            }
                        } else {
                            Self::create_new_cluster_for(cluster_manager, store, memory, stats).await;
                        }
                    }
                    AssignAction::CreateNew => {
                        Self::create_new_cluster_for(cluster_manager, store, memory, stats).await;
                    }
                }
            }
            Err(e) => {
                warn!(memory_id = %memory.id, error = %e, "incremental: assigner failed");
                stats.errors += 1;
            }
        }
    }

    async fn create_new_cluster_for(
        cluster_manager: &ClusterManager,
        store: &Arc<LanceStore>,
        memory: &Memory,
        stats: &mut ClusterStats,
    ) {
        match store.get_vector_by_id(&memory.id).await {
            Ok(Some(vector)) => {
                match cluster_manager.create_cluster(memory, &vector, memory.tags.clone()).await {
                    Ok(cluster) => {
                        if let Err(e) = store.update_memory_cluster_id(&memory.id, Some(&cluster.id), true).await {
                            warn!(memory_id = %memory.id, cluster_id = %cluster.id, error = %e, "incremental: failed to set anchor cluster_id");
                            stats.errors += 1;
                        } else {
                            debug!(memory_id = %memory.id, cluster_id = %cluster.id, "incremental: created new cluster");
                            stats.created_new_clusters += 1;
                            stats.processed += 1;
                        }
                    }
                    Err(e) => {
                        warn!(memory_id = %memory.id, error = %e, "incremental: create_cluster failed");
                        stats.errors += 1;
                    }
                }
            }
            Ok(None) => {
                warn!(memory_id = %memory.id, "incremental: no vector found for memory");
                stats.errors += 1;
            }
            Err(e) => {
                warn!(memory_id = %memory.id, error = %e, "incremental: get_vector failed");
                stats.errors += 1;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClusterStats {
    pub processed: usize,
    pub assigned_to_existing: usize,
    pub created_new_clusters: usize,
    pub errors: usize,
}
