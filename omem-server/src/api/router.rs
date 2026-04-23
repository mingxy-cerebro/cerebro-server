use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use crate::api::handlers;
use crate::api::middleware::{auth_middleware, logging_middleware};
use crate::api::server::AppState;

pub fn build_router(state: Arc<AppState>) -> Router {
    let authed_routes = Router::new()
        .route("/v1/memories/search", get(handlers::search_memories))
        .route("/v1/memories/batch-delete", post(handlers::batch_delete))
        .route("/v1/memories/batch-get", post(handlers::batch_get_memories))
        .route("/v1/memories/all", delete(handlers::delete_all_memories))
        .route(
            "/v1/memories/{id}",
            get(handlers::get_memory)
                .put(handlers::update_memory)
                .delete(handlers::delete_memory),
        )
        .route(
            "/v1/memories",
            get(handlers::list_memories).post(handlers::create_memory),
        )
        .route("/v1/profile", get(handlers::get_profile))
        .route("/v1/stats", get(handlers::get_stats))
        .route("/v1/stats/config", get(handlers::get_config))
        .route("/v1/stats/tags", get(handlers::get_tags))
        .route("/v1/stats/decay", get(handlers::get_decay))
        .route("/v1/stats/relations", get(handlers::get_relations))
        .route("/v1/stats/spaces", get(handlers::get_spaces_stats))
        .route("/v1/stats/sharing", get(handlers::get_sharing_stats))
        .route("/v1/stats/agents", get(handlers::get_agents_stats))
        .route("/v1/tier-changes", get(handlers::get_tier_changes))
        .route("/v1/tier-changes/delete", post(handlers::delete_tier_history_entry))
        .route("/v1/files", post(handlers::upload_file))
        .route(
            "/v1/imports",
            post(handlers::create_import).get(handlers::list_imports),
        )
        .route("/v1/imports/{id}", get(handlers::get_import))
        .route(
            "/v1/imports/{id}/intelligence",
            post(handlers::trigger_intelligence),
        )
        .route("/v1/imports/{id}/rollback", post(handlers::rollback_import))
        .route(
            "/v1/imports/cross-reconcile",
            post(handlers::cross_reconcile),
        )
        .route(
            "/v1/connectors/github/connect",
            post(handlers::github_connect),
        )
        .route(
            "/v1/spaces",
            get(handlers::list_spaces).post(handlers::create_space),
        )
        .route(
            "/v1/spaces/{id}",
            get(handlers::get_space)
                .put(handlers::update_space)
                .delete(handlers::delete_space),
        )
        .route("/v1/spaces/{id}/members", post(handlers::add_member))
        .route(
            "/v1/spaces/{id}/members/{user_id}",
            delete(handlers::remove_member).put(handlers::update_member_role),
        )
        .route("/v1/memories/{id}/share", post(handlers::share_memory))
        .route("/v1/memories/{id}/pull", post(handlers::pull_memory))
        .route("/v1/memories/{id}/unshare", post(handlers::unshare_memory))
        .route("/v1/memories/{id}/reshare", post(handlers::reshare_memory))
        .route("/v1/memories/batch-share", post(handlers::batch_share))
        .route("/v1/memories/share-all", post(handlers::share_all))
        .route(
            "/v1/memories/{id}/share-to-user",
            post(handlers::share_to_user),
        )
        .route(
            "/v1/memories/share-all-to-user",
            post(handlers::share_all_to_user),
        )
        .route("/v1/org/setup", post(handlers::org_setup))
        .route("/v1/org/{id}/publish", post(handlers::org_publish))
        .route(
            "/v1/spaces/{id}/auto-share-rules",
            get(handlers::list_auto_share_rules).post(handlers::create_auto_share_rule),
        )
        .route(
            "/v1/spaces/{id}/auto-share-rules/{rule_id}",
            delete(handlers::delete_auto_share_rule),
        )
        .route("/v1/vault/password", post(handlers::set_vault_password))
        .route("/v1/vault/verify", post(handlers::verify_vault_password))
        .route("/v1/vault/password", delete(handlers::delete_vault_password))
        .route("/v1/vault/status", get(handlers::get_vault_status))
        .route("/v1/should-recall", post(handlers::should_recall))
        .route(
            "/v1/session-recalls",
            get(handlers::list_session_recalls).post(handlers::create_session_recall),
        )
        .route(
            "/v1/session-recalls/{id}",
            get(handlers::get_session_recall).delete(handlers::delete_session_recall),
        )
        .route("/v1/clusters", get(handlers::list_clusters))
        .route("/v1/clusters/trigger", post(handlers::trigger_clustering))
        .route("/v1/clusters/jobs", get(handlers::list_clustering_jobs))
        .route("/v1/clusters/jobs/{id}", get(handlers::get_clustering_job))
        .route("/v1/clusters/stats", get(handlers::get_clustering_stats))
        .route("/v1/memories/re-embed", post(handlers::reembed_memories))
        .route("/v1/clusters/{id}", get(handlers::get_cluster))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let public_routes = Router::new()
        .route("/health", get(health))
        .route("/v1/tenants", post(handlers::create_tenant))
        .route("/v1/tenants/{id}", get(handlers::get_tenant))
        .route(
            "/v1/connectors/github/webhook",
            post(handlers::github_webhook),
        );

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .merge(authed_routes)
        .merge(public_routes)
        .layer(cors)
        .layer(axum::middleware::from_fn(logging_middleware))
        .with_state(state)
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"status": "ok"}))
}
