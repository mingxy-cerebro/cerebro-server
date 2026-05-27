use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use tracing::warn;

use crate::api::server::AppState;
use crate::domain::error::OmemError;
use crate::lifecycle::decay::DecayConfig;
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
    let decay_config = state.config.decay_config();
    let tier_config = state.config.tier_config();
    let tier_manager = TierManager::from_config(tier_config, decay_config.clone());
    let stores = state.store_manager.cached_stores().await;

    if stores.is_empty() {
        return;
    }

    for store in &stores {
        evaluate_tiers(store, &tier_manager).await;
        let _removed = run_forgetting(store, &state, decay_config.clone()).await;
    }

    for store in &stores {
        if let Err(e) = store.optimize().await {
            warn!(error = %e, "lifecycle_optimize_failed");
        }
    }
}

async fn run_forgetting(store: &Arc<crate::store::LanceStore>, state: &Arc<AppState>, decay_config: DecayConfig) -> Vec<crate::domain::memory::Memory> {
    let forgetter = AutoForgetter::new(
        store.clone(),
        decay_config,
        state.config.forgetting_max_stale_deletions,
        state.config.forgetting_access_count_protection,
        state.config.forgetting_superseded_archive_days,
    );
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

    match forgetter.archive_superseded().await {
        Ok(archived) if !archived.is_empty() => {
            tracing::info!(archived = archived.len(), "lifecycle_superseded_archive");
            removed.extend(archived);
        }
        Err(e) => {
            warn!(error = %e, "lifecycle_archive_superseded_failed");
        }
        _ => {}
    }

    match forgetter.cleanup_stale().await {
        Ok(stale) => {
            if !stale.is_empty() {
                tracing::info!(count = stale.len(), "lifecycle_cleanup_stale_complete");
                removed.extend(stale);
            }
        }
        Err(e) => {
            warn!(error = %e, "lifecycle_cleanup_stale_failed");
        }
    }

    removed
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

        // Private memories are protected from tier demotion
        if memory.visibility == "private" {
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
        if demoted_count >= 10 {
            tracing::warn!(
                demoted = demoted_count,
                note = "First lifecycle run after upgrade may cause batch demotion of memories previously protected by floor clamping",
                "lifecycle_tier_evaluation_complete_large_batch"
            );
        } else {
            tracing::info!(demoted = demoted_count, "lifecycle_tier_evaluation_complete");
        }
    }
}
