use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

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
        }
    }

    pub fn with_llm(mut self, llm: Arc<dyn crate::llm::LlmService>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub async fn run(self: Arc<Self>) {
        if self.run_on_start {
            info!("clustering_scheduler_running_on_start");
            if let Err(e) = self.run_once().await {
                warn!(error = %e, "clustering_scheduler_initial_run_failed");
            }
        }
        let mut interval = tokio::time::interval(self.interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.run_once().await {
                warn!(error = %e, "clustering_scheduler_run_failed");
            }
        }
    }

    pub async fn run_once(&self,
    ) -> Result<(), crate::domain::error::OmemError> {
        let stores = self.store_manager.cached_stores().await;

        if stores.is_empty() {
            return Ok(());
        }

        for store in &stores {
            self.run_clustering(store).await;
        }

        Ok(())
    }

    async fn run_clustering(
        &self,
        store: &Arc<crate::store::LanceStore>,
    ) {
        let clusterer = BackgroundClusterer::new(
            store.clone(),
            self.cluster_store.clone(),
            self.embed.clone(),
            self.llm.clone(),
        );

        let clusterer = match clusterer.await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "scheduler_failed_to_create_clusterer");
                return;
            }
        };

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