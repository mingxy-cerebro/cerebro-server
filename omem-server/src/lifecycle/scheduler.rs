use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tracing::{info, warn};

use crate::api::event_bus::{ServerEvent, SharedEventBus};
use crate::api::scheduler_control::SharedSchedulerControl;
use crate::cluster::cluster_store::ClusterStore;
use crate::cluster::manager::ClusterManager;
use crate::domain::memory::Memory;
use crate::lifecycle::forgetting::AutoForgetter;
use crate::lifecycle::tier::TierManager;
use crate::store::StoreManager;

pub struct LifecycleScheduler {
    store_manager: Arc<StoreManager>,
    cluster_store: Arc<ClusterStore>,
    embed: Option<Arc<dyn crate::embed::EmbedService>>,
    llm: Option<Arc<dyn crate::llm::LlmService>>,
    interval_secs: u64,
    run_on_start: bool,
    #[allow(dead_code)]
    max_memories_per_store: usize,
    event_bus: Option<SharedEventBus>,
    scheduler_control: Option<SharedSchedulerControl>,
}

impl LifecycleScheduler {
    pub fn new(
        store_manager: Arc<StoreManager>,
        cluster_store: Arc<ClusterStore>,
        interval: Duration,
        run_on_start: bool,
    ) -> Self {
        Self {
            store_manager,
            cluster_store,
            embed: None,
            llm: None,
            interval_secs: interval.as_secs(),
            run_on_start,
            max_memories_per_store: 5000,
            event_bus: None,
            scheduler_control: None,
        }
    }

    pub fn with_event_bus(mut self, bus: SharedEventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_scheduler_control(mut self, ctrl: SharedSchedulerControl) -> Self {
        self.scheduler_control = Some(ctrl);
        self
    }

    fn emit(&self, event_type: &str, tenant_id: &str, data: serde_json::Value) {
        if let Some(bus) = &self.event_bus {
            bus.publish(ServerEvent {
                event_type: event_type.to_string(),
                tenant_id: tenant_id.to_string(),
                data: Some(data),
                timestamp: Utc::now().to_rfc3339(),
            });
        }
    }

    pub fn with_services(
        mut self,
        embed: Arc<dyn crate::embed::EmbedService>,
        llm: Option<Arc<dyn crate::llm::LlmService>>,
    ) -> Self {
        self.embed = Some(embed);
        self.llm = llm;
        self
    }

    pub async fn run(self: Arc<Self>) {
        if self.run_on_start {
            info!("lifecycle_scheduler_running_on_start");
            if let Err(e) = self.run_once().await {
                warn!(error = %e, "lifecycle_scheduler_initial_run_failed");
            }
        }
        loop {
            let delay = self.next_run_delay();
            info!(
                "lifecycle_scheduler_next_run_in_seconds={}",
                delay.as_secs()
            );
            tokio::time::sleep(delay).await;

            if let Some(ctrl) = &self.scheduler_control {
                if ctrl.is_lifecycle_paused() {
                    info!("lifecycle_scheduler_paused_skipping");
                    continue;
                }
            }

            if let Err(e) = self.run_once().await {
                warn!(error = %e, "lifecycle_scheduler_run_failed");
            }
        }
    }

    fn next_run_delay(&self) -> Duration {
        if self.interval_secs > 0 {
            // Legacy interval mode (fallback)
            Duration::from_secs(self.interval_secs)
        } else {
            // Daily at midnight (Asia/Shanghai = UTC+8)
            let now_utc = Utc::now();
            let now_utc_ts = now_utc.timestamp();

            let shanghai_offset_secs = 28800;
            let now_shanghai_ts = now_utc_ts + shanghai_offset_secs;
            let seconds_in_day = 86400;
            let next_midnight_shanghai_ts = ((now_shanghai_ts / seconds_in_day) + 1) * seconds_in_day;
            let next_midnight_utc_ts = next_midnight_shanghai_ts - shanghai_offset_secs;
            let secs_until = (next_midnight_utc_ts - now_utc_ts).max(60) as u64;

            Duration::from_secs(secs_until)
        }
    }

    async fn run_once(&self) -> Result<(), crate::domain::error::OmemError> {
        if let Some(ctrl) = &self.scheduler_control {
            ctrl.set_lifecycle_running(true);
        }
        let result = self.run_once_inner().await;
        if let Some(ctrl) = &self.scheduler_control {
            ctrl.set_lifecycle_running(false);
        }
        result
    }

    async fn run_once_inner(&self) -> Result<(), crate::domain::error::OmemError> {
        let tier_manager = TierManager::with_defaults();
        let stores = self.store_manager.cached_stores().await;

        if stores.is_empty() {
            return Ok(());
        }

        self.emit("lifecycle.started", "system", json!({"stores": stores.len()}));

        for (i, store) in stores.iter().enumerate() {
            self.emit("lifecycle.stage", "system", json!({"phase": "tier_evaluation", "store_index": i, "total_stores": stores.len()}));
            self.evaluate_tiers(store, &tier_manager).await;

            self.emit("lifecycle.stage", "system", json!({"phase": "forgetting", "store_index": i}));
            let removed = self.run_forgetting(store).await;

            if !removed.is_empty() {
                self.emit("lifecycle.forgotten", "system", json!({
                    "count": removed.len(),
                    "memory_ids": removed.iter().map(|m| m.id.clone()).take(20).collect::<Vec<_>>()
                }));
            }

            self.emit("lifecycle.stage", "system", json!({"phase": "orphan_cleanup", "store_index": i}));
            self.cleanup_orphan_clusters(store, &removed).await;
        }

        for store in &stores {
            if let Err(e) = store.optimize().await {
                warn!(error = %e, "scheduler_optimize_failed");
            }
        }

        if let Err(e) = self.cluster_store.optimize().await {
            warn!(error = %e, "scheduler_cluster_optimize_failed");
        }

        self.emit("lifecycle.complete", "system", json!({"stores": stores.len()}));

        Ok(())
    }

    async fn run_forgetting(&self, store: &Arc<crate::store::LanceStore>) -> Vec<Memory> {
        let forgetter = AutoForgetter::new(store.clone());
        let mut removed = Vec::new();

        match forgetter.cleanup_expired().await {
            Ok(deleted) if !deleted.is_empty() => {
                info!(expired = deleted.len(), "scheduler_expired_cleanup");
                removed.extend(deleted);
            }
            Err(e) => {
                warn!(error = %e, "scheduler_cleanup_expired_failed");
            }
            _ => {}
        }

        match forgetter.archive_superseded(30).await {
            Ok(archived) if !archived.is_empty() => {
                info!(archived = archived.len(), "scheduler_superseded_archive");
                removed.extend(archived);
            }
            Err(e) => {
                warn!(error = %e, "scheduler_archive_superseded_failed");
            }
            _ => {}
        }

        removed
    }

    async fn cleanup_orphan_clusters(
        &self,
        _store: &Arc<crate::store::LanceStore>,
        removed_memories: &[Memory],
    ) {
        if removed_memories.is_empty() {
            return;
        }

        let manager = ClusterManager::new(self.cluster_store.clone(), self.llm.clone());

        for memory in removed_memories {
            if let Err(e) = manager.on_memory_removed(memory).await {
                warn!(memory_id = %memory.id, error = %e, "failed to maintain cluster on memory removal");
            }
        }

        match self.cluster_store.list_empty_clusters().await {
            Ok(empty) => {
                for cluster in empty {
                    if let Err(e) = self.cluster_store.delete_cluster(&cluster.id).await {
                        warn!(cluster_id = %cluster.id, error = %e, "failed to delete empty cluster");
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to list empty clusters");
            }
        }
    }

    async fn evaluate_tiers(
        &self,
        store: &Arc<crate::store::LanceStore>,
        tier_manager: &TierManager,
    ) {
        let batch_size = 100;
        let mut offset = 0;
        let mut demoted_count = 0usize;

        loop {
            let memories = match store.list(batch_size, offset).await {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, offset, "scheduler_failed_to_list_memories");
                    return;
                }
            };

            if memories.is_empty() {
                break;
            }

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
                        "tier_changed_by_scheduler"
                    );
                    memory.append_tier_change(&old_tier.to_string(), &new_tier.to_string(), "scheduled_evaluation");
                    memory.tier = new_tier;
                    if let Err(e) = store.update(&memory, None).await {
                        warn!(memory_id = %memory.id, error = %e, "scheduler_failed_to_update_tier");
                    }
                    demoted_count += 1;
                }
            }

            offset += batch_size;
        }

        if demoted_count > 0 {
            info!(demoted = demoted_count, "scheduler_tier_evaluation_complete");
        }
    }
}
