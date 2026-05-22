use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tracing::{info, warn};

use crate::api::event_bus::{ServerEvent, SharedEventBus};
use crate::api::scheduler_control::SharedSchedulerControl;
use crate::api::server::SessionLockMap;
use crate::domain::memory::Memory;
use crate::lifecycle::forgetting::AutoForgetter;
use crate::lifecycle::tier::TierManager;
use crate::lifecycle::decay::DecayConfig;
use crate::lifecycle::tier::TierConfig;
use crate::profile_v2::store::ProfileStore;
use crate::profile_v2::types::{PreferenceStatus, ProfileChangelog};
use crate::store::StoreManager;

pub struct LifecycleScheduler {
    store_manager: Arc<StoreManager>,
    embed: Option<Arc<dyn crate::embed::EmbedService>>,
    llm: Option<Arc<dyn crate::llm::LlmService>>,
    interval_secs: u64,
    run_on_start: bool,
    #[allow(dead_code)]
    max_memories_per_store: usize,
    event_bus: Option<SharedEventBus>,
    scheduler_control: Option<SharedSchedulerControl>,
    session_locks: Option<Arc<SessionLockMap>>,
    decay_config: DecayConfig,
    tier_config: TierConfig,
    forgetting_max_stale_deletions: usize,
    forgetting_access_count_protection: u32,
    forgetting_superseded_archive_days: u32,
    profile_store: Option<Arc<ProfileStore>>,
    profile_dormant_days: u32,
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
            interval_secs: interval.as_secs(),
            run_on_start,
            max_memories_per_store: 5000,
            event_bus: None,
            scheduler_control: None,
            session_locks: None,
            decay_config: DecayConfig::default(),
            tier_config: TierConfig::default(),
            forgetting_max_stale_deletions: 50,
            forgetting_access_count_protection: 5,
            forgetting_superseded_archive_days: 30,
            profile_store: None,
            profile_dormant_days: 90,
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

    pub fn with_session_locks(
        mut self,
        locks: Arc<SessionLockMap>,
    ) -> Self {
        self.session_locks = Some(locks);
        self
    }

    pub fn with_lifecycle_config(
        mut self,
        decay_config: DecayConfig,
        tier_config: TierConfig,
        max_stale_deletions: usize,
        access_count_protection: u32,
        superseded_archive_days: u32,
    ) -> Self {
        self.decay_config = decay_config;
        self.tier_config = tier_config;
        self.forgetting_max_stale_deletions = max_stale_deletions;
        self.forgetting_access_count_protection = access_count_protection;
        self.forgetting_superseded_archive_days = superseded_archive_days;
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

    pub fn with_profile_store(mut self, store: Arc<ProfileStore>, dormant_days: u32) -> Self {
        self.profile_store = Some(store);
        self.profile_dormant_days = dormant_days;
        self
    }

    pub async fn run(self: Arc<Self>) {
        if self.run_on_start {
            info!("lifecycle_scheduler_running_on_start");
            if let Err(e) = self.run_once().await {
                warn!(error = %e, "lifecycle_scheduler_initial_run_failed");
            }

            // Cleanup expired induction locks on start
            if let Some(ps) = &self.profile_store {
                if let Err(e) = ps.cleanup_expired_locks() {
                    warn!(error = %e, "profile_locks_cleanup_failed");
                }
            }
        }

        // Spawn background session-lock cleanup daemon — runs every 60s
        let prune_self = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                if let Some(locks) = &prune_self.session_locks {
                    let before = locks.len();
                    locks.retain(|_, (_, last_used)| last_used.elapsed() < Duration::from_secs(86400));
                    let after = locks.len();
                    if before != after {
                        info!(pruned = before - after, "session_locks_pruned");
                    }
                }
            }
        });

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
        let tier_manager = TierManager::from_config(self.tier_config.clone(), self.decay_config.clone());
        let stores = self.store_manager.all_stores().await?;

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
        }

        for store in &stores {
            if let Err(e) = store.optimize().await {
                warn!(error = %e, "scheduler_optimize_failed");
            }
        }

        let session_optimized = self.store_manager.optimize_session_stores().await;
        info!(session_stores_optimized = session_optimized, "scheduler_session_optimize_done");

        self.emit("lifecycle.complete", "system", json!({"stores": stores.len()}));

        // Profile V2 maintenance tasks
        if let Some(ps) = &self.profile_store {
            if let Err(e) = self.check_dormant_preferences(ps).await {
                warn!(error = %e, "profile_dormant_check_failed");
            }
            if let Err(e) = self.cleanup_deleted_preferences(ps).await {
                warn!(error = %e, "profile_deleted_cleanup_failed");
            }
            if let Err(e) = ps.cleanup_expired_locks() {
                warn!(error = %e, "profile_locks_cleanup_failed");
            }
        }

        Ok(())
    }

    async fn run_forgetting(&self, store: &Arc<crate::store::LanceStore>) -> Vec<Memory> {
        let forgetter = AutoForgetter::new(store.clone(), self.decay_config.clone(), self.forgetting_max_stale_deletions, self.forgetting_access_count_protection, self.forgetting_superseded_archive_days);
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

        match forgetter.archive_superseded().await {
            Ok(archived) if !archived.is_empty() => {
                info!(archived = archived.len(), "scheduler_superseded_archive");
                removed.extend(archived);
            }
            Err(e) => {
                warn!(error = %e, "scheduler_archive_superseded_failed");
            }
            _ => {}
        }

        match forgetter.cleanup_stale().await {
            Ok(stale) => {
                if !stale.is_empty() {
                    info!(count = stale.len(), "scheduler_cleanup_stale_complete");
                    removed.extend(stale);
                }
            }
            Err(e) => {
                warn!(error = %e, "scheduler_cleanup_stale_failed");
            }
        }

        removed
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

    async fn check_dormant_preferences(&self, store: &ProfileStore) -> Result<(), crate::domain::error::OmemError> {
        let stores = self.store_manager.all_stores().await?;
        let dormant_threshold = chrono::Utc::now() - chrono::Duration::days(self.profile_dormant_days as i64);

        for lance_store in &stores {
            let tenant_id = match lance_store.list(1, 0).await {
                Ok(memories) => memories.first().map(|m| m.tenant_id.clone()).unwrap_or_default(),
                Err(_) => String::new(),
            };
            if tenant_id.is_empty() {
                continue;
            }

            let prefs = store.get_preferences(&tenant_id, None)?;
            let mut dormant_count = 0usize;
            for pref in &prefs {
                if pref.status == PreferenceStatus::Active || pref.status == PreferenceStatus::Reinforce {
                    if pref.last_reinforced_at < dormant_threshold {
                        store.update_status(&pref.id, "dormant")?;
                        store.record_changelog(&ProfileChangelog {
                            id: uuid::Uuid::new_v4().to_string(),
                            tenant_id: tenant_id.clone(),
                            preference_id: pref.id.clone(),
                            action: "dormant".to_string(),
                            old_value: Some(pref.status.as_str().to_string()),
                            new_value: None,
                            source: "lifecycle".to_string(),
                            created_at: chrono::Utc::now(),
                        })?;
                        dormant_count += 1;
                    }
                }
            }
            if dormant_count > 0 {
                info!(tenant_id, dormant_count, "profile_preferences_marked_dormant");
                store.invalidate_cache(&tenant_id);
            }
        }
        Ok(())
    }

    async fn cleanup_deleted_preferences(&self, store: &ProfileStore) -> Result<(), crate::domain::error::OmemError> {
        let stores = self.store_manager.all_stores().await?;
        let deleted_threshold = chrono::Utc::now() - chrono::Duration::days(180);

        for lance_store in &stores {
            let tenant_id = match lance_store.list(1, 0).await {
                Ok(memories) => memories.first().map(|m| m.tenant_id.clone()).unwrap_or_default(),
                Err(_) => String::new(),
            };
            if tenant_id.is_empty() {
                continue;
            }

            let prefs = store.get_preferences(&tenant_id, None)?;
            let mut deleted_count = 0usize;

            for pref in &prefs {
                if pref.status == PreferenceStatus::Dormant && pref.updated_at < deleted_threshold {
                    store.record_changelog(&ProfileChangelog {
                        id: uuid::Uuid::new_v4().to_string(),
                        tenant_id: tenant_id.clone(),
                        preference_id: pref.id.clone(),
                        action: "deleted".to_string(),
                        old_value: Some("dormant".to_string()),
                        new_value: None,
                        source: "lifecycle_cleanup".to_string(),
                        created_at: chrono::Utc::now(),
                    })?;
                    store.delete_preference(&pref.id)?;
                    deleted_count += 1;
                }
            }
            if deleted_count > 0 {
                info!(tenant_id, deleted_count, "profile_preferences_hard_deleted");
                store.invalidate_cache(&tenant_id);
            }
        }
        Ok(())
    }
}
