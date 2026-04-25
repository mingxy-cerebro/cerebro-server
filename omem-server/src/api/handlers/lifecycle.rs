use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use tracing::warn;

use crate::api::server::AppState;
use crate::cluster::manager::ClusterManager;
use crate::domain::error::OmemError;
use crate::lifecycle::forgetting::AutoForgetter;
use crate::lifecycle::tier::TierManager;

#[derive(Serialize)]
pub struct TriggerLifecycleResponse {
    pub status: String,
    pub message: String,
}

pub async fn trigger_lifecycle(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TriggerLifecycleResponse>, OmemError> {
    let stores = state.store_manager.cached_stores().await;

    if stores.is_empty() {
        return Ok(Json(TriggerLifecycleResponse {
            status: "skipped".to_string(),
            message: "No stores to process".to_string(),
        }));
    }

    tokio::spawn(async move {
        run_lifecycle_cycle(state).await;
    });

    Ok(Json(TriggerLifecycleResponse {
        status: "triggered".to_string(),
        message: format!("Lifecycle cycle triggered for {} stores", stores.len()),
    }))
}

async fn run_lifecycle_cycle(state: Arc<AppState>) {
    let tier_manager = TierManager::with_defaults();
    let stores = state.store_manager.cached_stores().await;

    if stores.is_empty() {
        return;
    }

    for store in &stores {
        evaluate_tiers(store, &tier_manager).await;
        let removed = run_forgetting(store).await;
        cleanup_orphan_clusters(&state, &removed).await;
    }

    for store in &stores {
        if let Err(e) = store.optimize().await {
            warn!(error = %e, "lifecycle_optimize_failed");
        }
    }

    if let Err(e) = state.cluster_store.optimize().await {
        warn!(error = %e, "lifecycle_cluster_optimize_failed");
    }
}

async fn run_forgetting(store: &Arc<crate::store::LanceStore>) -> Vec<crate::domain::memory::Memory> {
    let forgetter = AutoForgetter::new(store.clone());
    let mut removed = Vec::new();

    match forgetter.cleanup_expired().await {
        Ok(deleted) if !deleted.is_empty() => {
            tracing::info!(expired = deleted.len(), "lifecycle_expired_cleanup");
            removed.extend(deleted);
        }
        Err(e) => {
            warn!(error = %e, "lifecycle_cleanup_expired_failed");
        }
        _ => {}
    }

    match forgetter.archive_superseded(30).await {
        Ok(archived) if !archived.is_empty() => {
            tracing::info!(archived = archived.len(), "lifecycle_superseded_archive");
            removed.extend(archived);
        }
        Err(e) => {
            warn!(error = %e, "lifecycle_archive_superseded_failed");
        }
        _ => {}
    }

    removed
}

async fn cleanup_orphan_clusters(
    state: &Arc<AppState>,
    removed_memories: &[crate::domain::memory::Memory],
) {
    if removed_memories.is_empty() {
        return;
    }

    let manager = ClusterManager::new(state.cluster_store.clone(), Some(state.llm.clone()));

    for memory in removed_memories {
        if let Err(e) = manager.on_memory_removed(memory).await {
            warn!(
                memory_id = %memory.id,
                error = %e,
                "failed_to_maintain_cluster_on_memory_removal"
            );
        }
    }

    match state.cluster_store.list_empty_clusters().await {
        Ok(empty) => {
            for cluster in empty {
                if let Err(e) = state.cluster_store.delete_cluster(&cluster.id).await {
                    warn!(cluster_id = %cluster.id, error = %e, "failed_to_delete_empty_cluster");
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "failed_to_list_empty_clusters");
        }
    }
}

async fn evaluate_tiers(
    store: &Arc<crate::store::LanceStore>,
    tier_manager: &TierManager,
) {
    const MAX_MEMORIES_PER_STORE: usize = 5000;

    let memories = match store.list(MAX_MEMORIES_PER_STORE, 0).await {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "lifecycle_failed_to_list_memories");
            return;
        }
    };

    let mut demoted_count = 0usize;
    for mut memory in memories {
        if memory.state != crate::domain::types::MemoryState::Active {
            continue;
        }

        let old_tier = memory.tier.clone();
        let new_tier = tier_manager.evaluate_tier(&memory);

        if new_tier != old_tier {
            tracing::info!(
                memory_id = %memory.id,
                old_tier = %old_tier,
                new_tier = %new_tier,
                access_count = memory.access_count,
                "tier_changed_by_lifecycle"
            );
            memory.append_tier_change(&old_tier.to_string(), &new_tier.to_string(), "lifecycle_trigger");
            memory.tier = new_tier;
            if let Err(e) = store.update(&memory, None).await {
                warn!(memory_id = %memory.id, error = %e, "lifecycle_failed_to_update_tier");
            }
            demoted_count += 1;
        }
    }

    if demoted_count > 0 {
        tracing::info!(demoted = demoted_count, "lifecycle_tier_evaluation_complete");
    }
}
