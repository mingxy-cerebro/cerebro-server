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
        "interval_secs": ctrl.interval_secs(),
        "last_run_at": ctrl.last_run_at().map(|dt| dt.to_rfc3339()),
        "next_run_eta": ctrl.next_run_eta().map(|dt| dt.to_rfc3339()),
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
