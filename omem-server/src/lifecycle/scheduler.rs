use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::cluster::cluster_store::ClusterStore;
use crate::cluster::manager::ClusterManager;
use crate::domain::memory::Memory;
use crate::lifecycle::forgetting::AutoForgetter;
use crate::lifecycle::tier::TierManager;
use crate::store::StoreManager;

pub struct LifecycleScheduler {
    store_manager: Arc<StoreManager>,
    embed: Option<Arc<dyn crate::embed::EmbedService>>,
    llm: Option<Arc<dyn crate::llm::LlmService>>,
    interval: Duration,
    run_on_start: bool,
    max_memories_per_store: usize,
}

impl LifecycleScheduler {
    pub fn new(
        store_manager: Arc<StoreManager>,
        interval: Duration,
        run_on_start: bool,
    ) -> Self {
        Self {
            store_manager,
            embed: None,
            llm: None,
            interval,
            run_on_start,
            max_memories_per_store: 5000,
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
        let mut interval = tokio::time::interval(self.interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.run_once().await {
                warn!(error = %e, "lifecycle_scheduler_run_failed");
            }
        }
    }

    async fn run_once(&self) -> Result<(), crate::domain::error::OmemError> {
        let tier_manager = TierManager::with_defaults();
        let stores = self.store_manager.cached_stores().await;

        if stores.is_empty() {
            return Ok(());
        }

        for store in &stores {
            self.evaluate_tiers(store, &tier_manager).await;
            let removed = self.run_forgetting(store).await;
            self.cleanup_orphan_clusters(store, &removed).await;
        }

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
        store: &Arc<crate::store::LanceStore>,
        removed_memories: &[Memory],
    ) {
        if removed_memories.is_empty() {
            return;
        }

        let cluster_store = match ClusterStore::new(store.db()).await {
            Ok(cs) => Arc::new(cs),
            Err(_) => return,
        };
        let manager = ClusterManager::new(cluster_store.clone());

        for memory in removed_memories {
            if let Err(e) = manager.on_memory_removed(memory).await {
                warn!(memory_id = %memory.id, error = %e, "failed to maintain cluster on memory removal");
            }
        }

        match cluster_store.list_empty_clusters().await {
            Ok(empty) => {
                for cluster in empty {
                    if let Err(e) = cluster_store.delete_cluster(&cluster.id).await {
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
        let memories = match store.list(self.max_memories_per_store, 0).await {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "scheduler_failed_to_list_memories");
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

        if demoted_count > 0 {
            info!(demoted = demoted_count, "scheduler_tier_evaluation_complete");
        }
    }
}
