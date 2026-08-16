use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use crate::api::server::AppState;
use crate::domain::error::OmemError;
use crate::domain::tenant::AuthInfo;
use crate::dream::{validate_request, DreamJob, DreamRequest};

/// POST /v1/dreams — 受理一次记忆整理任务,立即返回 202 + job(SPEC.md §5.1)。
pub async fn create_dream(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<DreamRequest>,
) -> Result<impl IntoResponse, OmemError> {
    validate_request(&body)?;

    let job = state.dream_jobs.spawn(
        state.dream_llm.clone(),
        body,
        auth.tenant_id,
        state.event_bus.clone(),
    )?;

    Ok((StatusCode::ACCEPTED, Json(job)))
}

/// GET /v1/dreams/{id} — 轮询 job 状态;非本人 job 当不存在(404,D-2)。
pub async fn get_dream(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<Json<DreamJob>, OmemError> {
    state
        .dream_jobs
        .get(&id, &auth.tenant_id)
        .map(Json)
        .ok_or_else(|| OmemError::NotFound(format!("dream job {id}")))
}
