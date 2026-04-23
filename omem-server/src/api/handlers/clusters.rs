use std::sync::Arc;

use axum::extract::{Extension, Json, Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::api::server::AppState;
use crate::domain::tenant::AuthInfo;
use crate::cluster::background_clustering::BackgroundClusterer;
use crate::domain::cluster::{ClusteringJob, ClusteringJobStatus};
use crate::domain::error::OmemError;

#[derive(Debug, Deserialize)]
pub struct TriggerClusteringRequest {
    pub space_id: Option<String>,
    pub batch_size: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TriggerClusteringResponse {
    pub job_id: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ClusteringJobResponse {
    pub job: ClusteringJob,
}

#[derive(Debug, Serialize)]
pub struct ClusteringJobsListResponse {
    pub jobs: Vec<ClusteringJob>,
}

#[derive(Debug, Serialize)]
pub struct ClusteringStatsResponse {
    pub total_clusters: u64,
    pub total_memories_in_clusters: u64,
    pub orphaned_memories: u64,
    pub recent_jobs: Vec<ClusteringJob>,
}

pub async fn trigger_clustering(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<TriggerClusteringRequest>,
) -> Result<(StatusCode, Json<TriggerClusteringResponse>), OmemError> {
    let space_id = body.space_id.unwrap_or_else(|| format!("personal/{}", auth.tenant_id));
    let batch_size = body.batch_size.unwrap_or(50);

    info!(
        tenant_id = %auth.tenant_id,
        space_id = %space_id,
        batch_size,
        "triggering background clustering"
    );

    let store = state
        .store_manager
        .get_store(&space_id)
        .await?;

    let cluster_store = crate::cluster::cluster_store::ClusterStore::new(store.db())
        .await
        .map_err(|e| OmemError::Storage(format!("failed to init cluster store: {e}")))?;

    let clusterer = BackgroundClusterer::new(
        store.clone(),
        Arc::new(cluster_store),
        state.embed.clone(),
        Some(state.llm.clone()),
    ).await?;

    let memories = store.list_all_active().await?;
    let total = memories.len() as u64;

    let mut job = ClusteringJob::new(&auth.tenant_id, &space_id, total);
    job.status = ClusteringJobStatus::Running;
    job.started_at = Some(chrono::Utc::now().to_rfc3339());

        let job_id = job.id.clone();
        tokio::spawn(async move {
            match clusterer.cluster_all_unassigned(batch_size).await {
            Ok(stats) => {
                info!(
                    job_id = %job_id,
                    processed = stats.processed,
                    assigned = stats.assigned_to_existing,
                    created = stats.created_new_clusters,
                    "clustering completed"
                );
            }
            Err(e) => {
                warn!(job_id = %job_id, error = %e, "clustering failed");
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(TriggerClusteringResponse {
            job_id: job.id.clone(),
            status: "running".to_string(),
            message: format!("Clustering job started for {} memories", total),
        }),
    ))
}

pub async fn get_clustering_job(
    State(state): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthInfo>,
    Path(job_id): Path<String>,
) -> Result<Json<ClusteringJobResponse>, OmemError> {
    match state.cluster_store.get_job(&job_id).await? {
        Some(job) => Ok(Json(ClusteringJobResponse { job })),
        None => Err(OmemError::NotFound(format!("Job {} not found", job_id))),
    }
}

pub async fn list_clustering_jobs(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<ClusteringJobsListResponse>, OmemError> {
    let jobs = state.cluster_store.list_jobs(&auth.tenant_id, 50).await?;
    Ok(Json(ClusteringJobsListResponse { jobs }))
}

pub async fn get_clustering_stats(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<ClusteringStatsResponse>, OmemError> {
    let space_id = format!("personal/{}", auth.tenant_id);
    let store = state
        .store_manager
        .get_store(&space_id)
        .await?;

    let all_memories = store.list_all_active().await?;
    let total_memories = all_memories.len() as u64;
    
    let memories_in_clusters = all_memories
        .iter()
        .filter(|m| m.cluster_id.is_some())
        .count() as u64;
    
    let orphaned = total_memories - memories_in_clusters;

    // Count unique clusters
    let total_clusters = all_memories
        .iter()
        .filter_map(|m| m.cluster_id.as_ref())
        .collect::<std::collections::HashSet<_>>()
        .len() as u64;

    Ok(Json(ClusteringStatsResponse {
        total_clusters,
        total_memories_in_clusters: memories_in_clusters,
        orphaned_memories: orphaned,
        recent_jobs: vec![],
    }))
}