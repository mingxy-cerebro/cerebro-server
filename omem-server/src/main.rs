use std::sync::Arc;

use tracing_subscriber::{fmt, EnvFilter};

use omem_server::api::{build_router, personal_space_id, AppState};
use omem_server::config::OmemConfig;
use omem_server::embed::{create_embed_service, EmbedService};
use omem_server::lifecycle::scheduler::LifecycleScheduler;
use omem_server::llm::{create_dream_llm_service, create_llm_service, create_profile_llm_service, create_recall_llm_service, LlmService};
use omem_server::store::{SpaceStore, StoreManager, TenantStore};
use omem_server::domain::category::CategoryRegistry;
use omem_server::profile_v2::store::ProfileStore;
use omem_server::profile_v2::service::ProfileV2Service;
use omem_server::profile_v2::induction::InductionEngine;
use omem_server::profile_v2::injection::InjectionBuilder;
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

/// 存量清洗：把匹配白名单的伪项目记忆 project_path 置空（归全局）。
/// `omem-server --migrate-global [--path /home/xxx ...] [--dry-run]`
/// 路径来源 = CLI --path（可重复）∪ OMEM_GLOBAL_HOME_PATHS；dry-run 只列计数与预览不落库。
async fn migrate_global(config: &OmemConfig, cli_paths: Vec<String>, dry_run: bool) {
    let mut whitelist: Vec<String> = cli_paths
        .into_iter()
        .map(|p| p.trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    whitelist.extend(config.global_home_paths.iter().cloned());
    whitelist.dedup();
    if whitelist.is_empty() {
        eprintln!("no whitelist: pass --path <dir> (repeatable) or set OMEM_GLOBAL_HOME_PATHS");
        std::process::exit(2);
    }

    let base_uri = config.store_uri();
    let store_manager = Arc::new(StoreManager::new(&base_uri));
    let tenant_store = Arc::new(
        TenantStore::new(&format!("{}/_system", base_uri))
            .await
            .expect("failed to create TenantStore"),
    );
    let tenants = tenant_store.list_all().await.expect("failed to list tenants");

    for tenant in &tenants {
        let store = match store_manager
            .get_store(&personal_space_id(&tenant.id))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("tenant {}: skip (store error: {e})", tenant.id);
                continue;
            }
        };
        for path in &whitelist {
            // 尾斜杠变体一并洗（与归一化端的 trim_end 容忍对称），防漏洗
            let p = path.replace('\'', "''");
            let filter = format!("project_path = '{p}' OR project_path = '{p}/'");
            let count = match store.count_by_filter(&filter).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("tenant {}: `{path}` count failed: {e}", tenant.id);
                    continue;
                }
            };
            if count == 0 {
                continue;
            }
            if dry_run {
                println!("[dry-run] tenant {}: `{path}` → global: {count} memories", tenant.id);
                let preview = store
                    .list_filtered(
                        &omem_server::store::lancedb::ListFilter {
                            project_path: Some(path.clone()),
                            ..Default::default()
                        },
                        5,
                        0,
                    )
                    .await;
                if let Ok(mems) = preview {
                    for m in mems {
                        let head: String = m.content.chars().take(80).collect();
                        println!("    - [{}] {}", m.id, head);
                    }
                }
            } else {
                match store.nullify_project_path(&filter).await {
                    Ok(n) => println!("tenant {}: `{path}` → global: migrated {n} memories", tenant.id),
                    Err(e) => eprintln!("tenant {}: `{path}` migrate failed: {e}", tenant.id),
                }
            }
        }
    }
    if dry_run {
        println!("dry-run done — re-run without --dry-run to apply");
    } else {
        println!("migration done");
    }
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 一次性运维子命令：存量伪项目记忆清洗（issue #3 议题1b）。默认不跑，手动触发。
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--migrate-global") {
        let dry_run = args.iter().any(|a| a == "--dry-run");
        let cli_paths: Vec<String> = args
            .windows(2)
            .filter(|w| w[0] == "--path")
            .map(|w| w[1].clone())
            .filter(|v| {
                if v.starts_with("--") {
                    eprintln!("--path expects a value, got `{v}`");
                    std::process::exit(2);
                }
                true
            })
            .collect();
        let config = OmemConfig::from_env();
        migrate_global(&config, cli_paths, dry_run).await;
        return;
    }

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

    // Profile V2 initialization
    let profile_store = Arc::new(ProfileStore::new(sqlite_store.clone()));
    profile_store.init().expect("failed to init profile store");
    let profile_llm: Option<Arc<dyn LlmService>> = match create_profile_llm_service(&config).await {
        Ok(svc) => Some(Arc::from(svc)),
        Err(_) => None,
    };
    let profile_v2_service = Arc::new(ProfileV2Service::new(profile_store.clone(), profile_llm.clone(), &config));
    let induction_engine = Arc::new(InductionEngine::new(profile_v2_service.clone()));
    let injection_builder = Arc::new(InjectionBuilder::new(profile_v2_service.clone()));
    tracing::info!(
        profile_enabled = config.profile_enabled,
        "profile_v2_initialized"
    );

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

    let dream_llm: Option<Arc<dyn LlmService>> = match create_dream_llm_service(&config).await {
        Ok(svc) => svc.map(Arc::from),
        Err(e) => {
            tracing::warn!(error = %e, "dream LLM init failed — /v1/dreams unavailable");
            None
        }
    };
    if dream_llm.is_none() {
        tracing::warn!("dream LLM is Noop (key empty) — set OMEM_DREAM_LLM_* to enable /v1/dreams");
    }

    let state = Arc::new(AppState {
        store_manager,
        tenant_store,
        space_store,
        embed,
        llm,
        recall_llm,
        dream_llm,
        dream_jobs: Arc::new(omem_server::dream::DreamJobStore::new()),
        config: config.clone(),
        import_semaphore: Arc::new(tokio::sync::Semaphore::new(3)),
        reconcile_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        event_bus: Arc::new(omem_server::api::event_bus::EventBus::new()),
        scheduler_control: Arc::new(
    omem_server::api::scheduler_control::SchedulerControl::new()
        .with_interval(config.scheduler_interval_secs)
        .with_run_on_start(config.scheduler_run_on_start),
        ),
        session_locks: Arc::new(dashmap::DashMap::new()),
        reranker: omem_server::retrieve::reranker::Reranker::from_env(),
        ingest_semaphore: Arc::new(tokio::sync::Semaphore::new(10)),
        sqlite_store,
        category_registry,
        profile_v2_service,
        induction_engine,
        injection_builder,
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
            .with_profile_store(profile_store.clone(), config.profile_dormant_days)
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
