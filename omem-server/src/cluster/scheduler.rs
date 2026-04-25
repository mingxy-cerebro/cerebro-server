use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::api::event_bus::{EventBus, ServerEvent, SharedEventBus};
use crate::api::scheduler_control::SharedSchedulerControl;
use crate::cluster::background_clustering::BackgroundClusterer;
use crate::cluster::cluster_store::ClusterStore;
use crate::store::StoreManager;

pub struct ClusteringScheduler {
    store_manager: Arc<StoreManager>,
    cluster_store: Arc<ClusterStore>,
    embed: Arc<dyn crate::embed::EmbedService>,
    llm: Option<Arc<dyn crate::llm::LlmService>>,
    interval: Duration,
    run_on_start: bool,
    batch_size: usize,
    event_bus: Option<SharedEventBus>,
    scheduler_control: Option<SharedSchedulerControl>,
}

impl ClusteringScheduler {
    pub fn new(
        store_manager: Arc<StoreManager>,
        cluster_store: Arc<ClusterStore>,
        embed: Arc<dyn crate::embed::EmbedService>,
        interval: Duration,
        run_on_start: bool,
    ) -> Self {
        Self {
            store_manager,
            cluster_store,
            embed,
            llm: None,
            interval,
            run_on_start,
            batch_size: 50,
            event_bus: None,
            scheduler_control: None,
        }
    }

    pub fn with_llm(mut self, llm: Arc<dyn crate::llm::LlmService>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn with_event_bus(mut self, bus: SharedEventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_scheduler_control(mut self, ctrl: SharedSchedulerControl) -> Self {
        self.scheduler_control = Some(ctrl);
        self
    }

    pub async fn run(self: Arc<Self>) {
        match self.cluster_store.list_running_jobs().await {
            Ok(jobs) => {
                let now = chrono::Utc::now();
                for job in jobs {
                    if let Some(ref started_at) = job.started_at {
                        if let Ok(started) = chrono::DateTime::parse_from_rfc3339(started_at) {
                            let duration = now.signed_duration_since(started);
                            if duration.num_minutes() > 30 {
                                warn!(job_id = %job.id, duration_minutes = %duration.num_minutes(), "auto-cleaning zombie job");
                                let _ = self.cluster_store.update_job_status(
                                    &job.id,
                                    "failed",
                                    None,
                                    None,
                                    None,
                                    Some("Auto-cleaned: zombie job"),
                                ).await;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to list running jobs for zombie cleanup");
            }
        }

        if self.run_on_start {
            info!("clustering_scheduler_running_on_start");
            if let Err(e) = self.run_once().await {
                warn!(error = %e, "clustering_scheduler_initial_run_failed");
            }
        }
        let mut interval = tokio::time::interval(self.interval);
        loop {
            if let Some(ctrl) = &self.scheduler_control {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = ctrl.clustering_notify.notified() => {
                        interval.reset();
                    }
                }
            } else {
                interval.tick().await;
            }

            if let Some(ctrl) = &self.scheduler_control {
                if ctrl.is_clustering_paused() {
                    info!("clustering_scheduler_paused_skipping");
                    continue;
                }
            }

            if let Err(e) = self.run_once().await {
                warn!(error = %e, "clustering_scheduler_run_failed");
            }
        }
    }

    pub async fn run_once(&self) -> Result<(), crate::domain::error::OmemError> {
        if let Some(ctrl) = &self.scheduler_control {
            ctrl.set_clustering_running(true);
        }
        let result = self.run_once_inner().await;
        if let Some(ctrl) = &self.scheduler_control {
            ctrl.set_clustering_running(false);
        }
        result
    }

    async fn run_once_inner(&self) -> Result<(), crate::domain::error::OmemError> {
        let stores = self.store_manager.cached_stores().await;

        if stores.is_empty() {
            return Ok(());
        }

        for store in &stores {
            if let Some(ctrl) = &self.scheduler_control {
                if ctrl.is_clustering_paused() {
                    info!("clustering_paused_between_tenants_stopping");
                    break;
                }
            }
            self.run_clustering(store).await;
        }

        Ok(())
    }

    async fn run_clustering(
        &self,
        store: &Arc<crate::store::LanceStore>,
    ) {
        let mut clusterer = match BackgroundClusterer::new(
            store.clone(),
            self.cluster_store.clone(),
            self.embed.clone(),
            self.llm.clone(),
        ).await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "scheduler_failed_to_create_clusterer");
                return;
            }
        };

        if let Some(ctrl) = &self.scheduler_control {
            clusterer = clusterer.with_scheduler_control(ctrl.clone());
        }

        if let Some(bus) = &self.event_bus {
            clusterer.set_event_bus(bus.clone(), String::new());
        }

        match clusterer.cluster_all_unassigned(self.batch_size).await {
            Ok(stats) if stats.processed > 0 => {
                info!(
                    processed = stats.processed,
                    assigned = stats.assigned_to_existing,
                    created = stats.created_new_clusters,
                    "clustering_scheduler_complete"
                );
            }
            Err(e) => {
                warn!(error = %e, "clustering_scheduler_failed");
            }
            _ => {}
        }
    }
}