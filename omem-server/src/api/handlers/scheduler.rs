use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::api::server::AppState;

pub async fn get_scheduler_status(
    State(state): State<std::sync::Arc<AppState>>,
) -> Json<Value> {
    let ctrl = &state.scheduler_control;
    Json(json!({
        "lifecycle": {
            "paused": ctrl.is_lifecycle_paused(),
            "running": ctrl.lifecycle_running.load(std::sync::atomic::Ordering::Relaxed),
        },
        "clustering": {
            "paused": ctrl.is_clustering_paused(),
            "running": ctrl.clustering_running.load(std::sync::atomic::Ordering::Relaxed),
        }
    }))
}

pub async fn pause_lifecycle(
    State(state): State<std::sync::Arc<AppState>>,
) -> Json<Value> {
    state.scheduler_control.pause_lifecycle();
    Json(json!({"ok": true, "action": "lifecycle_paused"}))
}

pub async fn resume_lifecycle(
    State(state): State<std::sync::Arc<AppState>>,
) -> Json<Value> {
    state.scheduler_control.resume_lifecycle();
    Json(json!({"ok": true, "action": "lifecycle_resumed"}))
}

pub async fn pause_clustering(
    State(state): State<std::sync::Arc<AppState>>,
) -> Json<Value> {
    state.scheduler_control.pause_clustering();
    Json(json!({"ok": true, "action": "clustering_paused"}))
}

pub async fn resume_clustering(
    State(state): State<std::sync::Arc<AppState>>,
) -> Json<Value> {
    state.scheduler_control.resume_clustering();
    Json(json!({"ok": true, "action": "clustering_resumed"}))
}
