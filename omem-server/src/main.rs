use std::sync::Arc;

use tracing_subscriber::{fmt, EnvFilter};

use omem_server::api::{build_router, AppState};
use omem_server::config::OmemConfig;
use omem_server::embed::{create_embed_service, EmbedService};
use omem_server::cluster::cluster_store::ClusterStore;
use omem_server::lifecycle::scheduler::LifecycleScheduler;
use omem_server::llm::{create_llm_service, create_cluster_llm_service, create_recall_llm_service, LlmService};
use omem_server::store::{SpaceStore, StoreManager, TenantStore};
use omem_server::domain::category::CategoryRegistry;
use omem_server::store::sqlite::SqliteStore;
use omem_server::store::sqlite_schema;

#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn init_tracing(config: &OmemConfig) {
    let filter =
        EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .init();
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = OmemConfig::from_env();
    init_tracing(&config);

    tracing::info!(
        port = config.port,
        embed_provider = %config.embed_provider,
        llm_provider = %config.llm_provider,
        llm_model = %config.llm_model,
        "starting omem-server"
    );

    let base_uri = config.store_uri();
    let store_manager = Arc::new(StoreManager::new(&base_uri));

    let system_uri = format!("{}/_system", base_uri);
    let tenant_store = Arc::new(
        TenantStore::new(&system_uri)
            .await
            .expect("failed to create TenantStore"),
    );
    tenant_store
        .init_table()
        .await
        .expect("failed to init tenants table");
    store_manager.set_tenant_store(tenant_store.clone());

    let space_store = Arc::new(
        SpaceStore::new(&system_uri)
            .await
            .expect("failed to create SpaceStore"),
    );
    space_store
        .init_tables()
        .await
        .expect("failed to init spaces tables");

    let sqlite_path = format!("{}/_system/omem.db", base_uri);
    let sqlite_store = Arc::new(
        SqliteStore::new(&sqlite_path)
            .expect("failed to create SqliteStore"),
    );
    {
        let conn = sqlite_store.conn().lock().expect("sqlite lock");
        sqlite_schema::create_tables(&conn).expect("failed to create SQLite tables");
    }
    let category_registry = Arc::new(CategoryRegistry::new(sqlite_store.clone()));

    // Migration: seed categories for existing tenants
    match tenant_store.list_all().await {
        Ok(tenants) => {
            for tenant in &tenants {
                match category_registry.get_categories(&tenant.id) {
                    Ok(cats) if cats.is_empty() => {
                        match category_registry.seed_tenant(&tenant.id) {
                            Ok(_) => tracing::info!("Seeded categories for tenant: {}", tenant.id),
                            Err(e) => tracing::warn!("Failed to seed categories for tenant {}: {}", tenant.id, e),
                        }
                    }
                    Err(e) => tracing::warn!("Failed to check categories for tenant {}: {}", tenant.id, e),
                    _ => {} // already seeded
                }
            }
        }
        Err(e) => tracing::warn!("Failed to list tenants for category migration: {}", e),
    }

    let embed: Arc<dyn EmbedService> = Arc::from(
        create_embed_service(&config)
            .await
            .expect("failed to create embed service"),
    );

    let llm: Arc<dyn LlmService> = Arc::from(
        create_llm_service(&config)
            .await
            .expect("failed to create LLM service"),
    );

    let recall_llm: Arc<dyn LlmService> = Arc::from(
        create_recall_llm_service(&config)
            .await
            .expect("failed to create recall LLM service"),
    );

    let cluster_llm: Arc<dyn LlmService> = if !config.cluster_llm_provider.is_empty() {
        match create_cluster_llm_service(&config).await {
            Ok(svc) => Arc::from(svc),
            Err(e) => {
                tracing::warn!("Failed to create cluster_llm: {e}, falling back to primary llm");
                llm.clone()
            }
        }
    } else {
        llm.clone()
    };

    let cluster_store = Arc::new(
        ClusterStore::new(&lancedb::connect(&base_uri).execute().await.expect("db connect")
        )
        .await
        .expect("failed to create cluster store")
    );

    let state = Arc::new(AppState {
        store_manager,
        tenant_store,
        space_store,
        embed,
        llm,
        recall_llm,
        cluster_llm,
        cluster_store,
        config: config.clone(),
        import_semaphore: Arc::new(tokio::sync::Semaphore::new(3)),
        reconcile_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        event_bus: Arc::new(omem_server::api::event_bus::EventBus::new()),
        scheduler_control: Arc::new(omem_server::api::scheduler_control::SchedulerControl::new()),
        session_locks: Arc::new(dashmap::DashMap::new()),
        reranker: omem_server::retrieve::reranker::Reranker::from_env(),
        ingest_semaphore: Arc::new(tokio::sync::Semaphore::new(10)),
        profile_cache: Arc::new(dashmap::DashMap::new()),
        sqlite_store,
        category_registry,
    });

    let app = build_router(state.clone());

    {
        let sm = state.store_manager.clone();
        tokio::spawn(async move {
            let count = sm.optimize_all_on_disk().await;
            tracing::info!(spaces_optimized = count, "startup_lancedb_cleanup_done");
        });
    }

    {
        let scheduler_interval = std::time::Duration::from_secs(config.scheduler_interval_secs);
        let ctrl = state.scheduler_control.clone();
        
        let lifecycle_scheduler = Arc::new(
            LifecycleScheduler::new(
                state.store_manager.clone(),
                state.cluster_store.clone(),
                scheduler_interval,
                config.scheduler_run_on_start,
            )
            .with_event_bus(state.event_bus.clone())
            .with_scheduler_control(ctrl.clone())
            .with_session_locks(state.session_locks.clone())
            .with_lifecycle_config(
                config.decay_config(),
                config.tier_config(),
                config.forgetting_max_stale_deletions,
                config.forgetting_access_count_protection,
                config.forgetting_superseded_archive_days,
            )
            .with_services(state.embed.clone(), Some(state.llm.clone()))
        );
        tokio::spawn(async move { lifecycle_scheduler.run().await });
        tracing::info!(
            interval_secs = config.scheduler_interval_secs,
            run_on_start = config.scheduler_run_on_start,
            "lifecycle_scheduler_started"
        );
    }

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind TCP listener");

    tracing::info!(%addr, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
