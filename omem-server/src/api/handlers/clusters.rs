use std::sync::Arc;

use axum::extract::{Extension, Json, Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::api::server::AppState;
use crate::domain::tenant::AuthInfo;
use crate::cluster::background_clustering::BackgroundClusterer;
use crate::domain::cluster::{ClusteringJob, ClusteringJobStatus, MemoryCluster};
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

    let cluster_store = state.cluster_store.clone();

    let existing_jobs = cluster_store.list_jobs(&auth.tenant_id, 50).await?;
    let has_running = existing_jobs.iter().any(|j| matches!(j.status, ClusteringJobStatus::Running));
    if has_running {
        return Ok((
            StatusCode::CONFLICT,
            Json(TriggerClusteringResponse {
                job_id: "".to_string(),
                status: "conflict".to_string(),
                message: "A clustering job is already running".to_string(),
            }),
        ));
    }

    let memories = store.list_all_active(Some(5000)).await?;
    let total = memories.len() as u64;

    let mut job = ClusteringJob::new(&auth.tenant_id, &space_id, total);
    job.status = ClusteringJobStatus::Running;
    job.started_at = Some(chrono::Utc::now().to_rfc3339());

    cluster_store.create_job(&job).await?;

    let mut clusterer = BackgroundClusterer::new(
        store.clone(),
        cluster_store.clone(),
        state.embed.clone(),
        Some(state.llm.clone()),
    ).await?
    .with_event_bus(state.event_bus.clone(), auth.tenant_id.clone());

    // Wire scheduler control so pause actually works for triggered clustering
    clusterer = clusterer.with_scheduler_control(state.scheduler_control.clone());

        let job_id = job.id.clone();
        let tenant_id = auth.tenant_id.clone();
        let cluster_store_clone = cluster_store.clone();
        let event_bus = state.event_bus.clone();
        tokio::spawn(async move {
            let completion_event = match clusterer.cluster_all_unassigned(batch_size).await {
                Ok(stats) => {
                    info!(
                        job_id = %job_id,
                        processed = stats.processed,
                        assigned = stats.assigned_to_existing,
                        created = stats.created_new_clusters,
                        "clustering completed"
                    );
                    let _ = cluster_store_clone.update_job_status(
                        &job_id,
                        "completed",
                        Some(stats.processed as u64),
                        Some(stats.assigned_to_existing as u64),
                        Some(stats.created_new_clusters as u64),
                        None,
                    ).await;
                    Some(crate::api::event_bus::ServerEvent {
                        event_type: "cluster.complete".to_string(),
                        tenant_id: tenant_id.clone(),
                        data: Some(serde_json::json!({
                            "job_id": job_id,
                            "processed": stats.processed,
                            "assigned": stats.assigned_to_existing,
                            "created_new": stats.created_new_clusters,
                            "errors": stats.errors,
                        })),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    })
                }
                Err(e) => {
                    warn!(job_id = %job_id, error = %e, "clustering failed");
                    let _ = cluster_store_clone.update_job_status(
                        &job_id,
                        "failed",
                        None,
                        None,
                        None,
                        Some(&e.to_string()),
                    ).await;
                    Some(crate::api::event_bus::ServerEvent {
                        event_type: "cluster.failed".to_string(),
                        tenant_id: tenant_id.clone(),
                        data: Some(serde_json::json!({
                            "job_id": job_id,
                            "error": e.to_string(),
                        })),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    })
                }
            };
            if let Some(evt) = completion_event {
                event_bus.publish(evt);
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

    let all_memories = store.list_all_active(Some(5000)).await?;
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

    let recent_jobs = state.cluster_store.list_jobs(&auth.tenant_id, 10).await.unwrap_or_default();

    Ok(Json(ClusteringStatsResponse {
        total_clusters,
        total_memories_in_clusters: memories_in_clusters,
        orphaned_memories: orphaned,
        recent_jobs,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ListClustersQuery {
    pub space_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ListClustersResponse {
    pub clusters: Vec<MemoryCluster>,
    pub total: usize,
}

pub async fn list_clusters(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<ListClustersQuery>,
) -> Result<Json<ListClustersResponse>, OmemError> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    let mut clusters = state.cluster_store.list_clusters_by_tenant(&auth.tenant_id, limit, offset).await?;

    let total = state.cluster_store.count_clusters_by_tenant(&auth.tenant_id).await?;

    let space_id = format!("personal/{}", auth.tenant_id);
    let store = state.store_manager.get_store(&space_id).await?;
    let all_memories = store.list_all_active(Some(5000)).await?;

    let cluster_member_counts: std::collections::HashMap<&str, u32> = all_memories
        .iter()
        .filter_map(|m| m.cluster_id.as_deref())
        .fold(std::collections::HashMap::new(), |mut acc, cid| {
            *acc.entry(cid).or_insert(0) += 1;
            acc
        });

    for cluster in &mut clusters {
        if let Some(&count) = cluster_member_counts.get(cluster.id.as_str()) {
            cluster.member_count = count;
        } else {
            cluster.member_count = 0;
        }
    }

    Ok(Json(ListClustersResponse { clusters, total }))
}

#[derive(Debug, Serialize)]
pub struct ClusterDetailResponse {
    pub cluster: MemoryCluster,
    pub members: Vec<serde_json::Value>,
}

pub async fn get_cluster(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(cluster_id): Path<String>,
) -> Result<Json<ClusterDetailResponse>, OmemError> {
    let mut cluster = state.cluster_store.get_by_id(&cluster_id).await?
        .ok_or_else(|| OmemError::NotFound(format!("Cluster {} not found", cluster_id)))?;

    let space_id = format!("personal/{}", auth.tenant_id);
    let store = state.store_manager.get_store(&space_id).await?;
    let all_memories = store.list_all_active(Some(5000)).await?;

    let members: Vec<serde_json::Value> = all_memories
        .iter()
        .filter(|m| m.cluster_id.as_deref() == Some(&cluster_id))
        .map(|m| serde_json::json!({
            "id": m.id,
            "content": m.content,
            "category": m.category.to_string(),
            "importance": m.importance,
            "tier": m.tier.to_string(),
            "created_at": m.created_at,
        }))
        .collect();

    cluster.member_count = members.len() as u32;

    Ok(Json(ClusterDetailResponse { cluster, members }))
}

pub async fn delete_cluster(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(cluster_id): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let cluster = state.cluster_store.get_by_id(&cluster_id).await?
        .ok_or_else(|| OmemError::NotFound(format!("Cluster {} not found", cluster_id)))?;

    if cluster.tenant_id != auth.tenant_id {
        return Err(OmemError::Unauthorized("not your cluster".to_string()));
    }

    let space_id = format!("personal/{}", auth.tenant_id);
    let store = state.store_manager.get_store(&space_id).await?;
    let all_memories = store.list_all_active(None).await?;

    let mut unlinked = 0u32;
    for mem in &all_memories {
        if mem.cluster_id.as_deref() == Some(&cluster_id) {
            store.update_memory_cluster_id(&mem.id, None, false).await?;
            unlinked += 1;
        }
    }

    state.cluster_store.delete_cluster(&cluster_id).await?;

    Ok(Json(serde_json::json!({
        "deleted": cluster_id,
        "unlinked_memories": unlinked,
    })))
}

pub async fn delete_clustering_job(
    State(state): State<Arc<AppState>>,
    Extension(_auth): Extension<AuthInfo>,
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    match state.cluster_store.get_job(&job_id).await? {
        Some(_) => {
            state.cluster_store.delete_job(&job_id).await?;
            Ok(Json(serde_json::json!({"deleted": job_id})))
        }
        None => Err(OmemError::NotFound(format!("Job {} not found", job_id))),
    }
}

#[derive(Debug, Deserialize)]
pub struct BatchDeleteRequest {
    pub cluster_ids: Vec<String>,
}

pub async fn batch_delete_clusters(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<BatchDeleteRequest>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let space_id = format!("personal/{}", auth.tenant_id);
    let store = state.store_manager.get_store(&space_id).await?;
    let all_memories = store.list_all_active(None).await?;

    let mut unlinked = 0u32;
    for mem in &all_memories {
        if let Some(ref cid) = mem.cluster_id {
            if body.cluster_ids.contains(cid) {
                store.update_memory_cluster_id(&mem.id, None, false).await?;
                unlinked += 1;
            }
        }
    }

    let deleted = state.cluster_store.batch_delete_clusters(&body.cluster_ids).await?;

    Ok(Json(serde_json::json!({
        "deleted": deleted,
        "unlinked_memories": unlinked,
    })))
}

pub async fn delete_all_clusters(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let space_id = format!("personal/{}", auth.tenant_id);
    let store = state.store_manager.get_store(&space_id).await?;

    let unlinked = store.clear_all_cluster_ids().await?;

    let deleted = state.cluster_store.delete_all_clusters_by_tenant(&auth.tenant_id).await?;

    Ok(Json(serde_json::json!({
        "deleted": deleted,
        "unlinked_memories": unlinked,
    })))
}