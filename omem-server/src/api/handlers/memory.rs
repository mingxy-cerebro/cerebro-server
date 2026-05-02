use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::{personal_space_id, AppState};
use crate::domain::category::Category;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::tenant::AuthInfo;
use crate::domain::types::MemoryType;
use crate::ingest::types::{IngestMessage, IngestMode, IngestRequest};
use crate::ingest::IngestPipeline;

use crate::lifecycle::tier::TierManager;
use crate::retrieve::pipeline::SearchRequest;
use crate::retrieve::RetrievalPipeline;
use crate::store::lancedb::ListFilter;
use crate::store::StoreManager;

// ── Request / Response DTOs ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateMemoryBody {
    // Message-based ingest
    pub messages: Option<Vec<MessageDto>>,
    #[serde(default)]
    pub mode: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub entity_context: Option<String>,

    // Direct single memory creation
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub tier: Option<String>,
    pub scope: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct MessageDto {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub scope: Option<String>,
    pub min_score: Option<f32>,
    #[serde(default)]
    pub include_trace: bool,
    pub space: Option<String>,
    pub tags: Option<String>,
    pub source: Option<String>,
    pub agent_id: Option<String>,
    #[serde(default)]
    pub check_stale: bool,
}

const MAX_SEARCH_LIMIT: usize = 1000;

fn default_limit() -> usize {
    20
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub q: Option<String>,
    pub memory_type: Option<String>,
    pub state: Option<String>,
    pub category: Option<String>,
    pub tier: Option<String>,
    pub tags: Option<String>,
    pub visibility: Option<String>,
    #[serde(default = "default_sort")]
    pub sort: String,
    #[serde(default = "default_order")]
    pub order: String,
}

fn default_sort() -> String {
    "created_at".to_string()
}
fn default_order() -> String {
    "desc".to_string()
}

#[derive(Deserialize)]
pub struct UpdateMemoryBody {
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub state: Option<String>,
    pub tier: Option<String>,
    pub tier_history: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResultDto {
    pub memory: Memory,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_info: Option<StaleInfo>,
}

#[derive(Serialize)]
pub struct SearchResponseDto {
    pub results: Vec<SearchResultDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<serde_json::Value>,
}

#[derive(Serialize, Clone)]
pub struct StaleInfo {
    pub is_stale: bool,
    pub source_version: Option<u64>,
    pub current_source_version: Option<u64>,
    pub source_deleted: bool,
}

#[derive(Deserialize)]
pub struct GetMemoryQuery {
    #[serde(default)]
    pub check_stale: bool,
    #[serde(default)]
    pub skip_access: bool,
}

#[derive(Serialize)]
pub struct ListResponseDto {
    pub memories: Vec<Memory>,
    pub total_count: usize,
    pub limit: usize,
    pub offset: usize,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// POST /v1/memories
///
/// Two modes:
/// - If `messages` present → ingest pipeline (async), returns 202
/// - If `content` present → create single pinned memory, returns 201
pub async fn create_memory(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<CreateMemoryBody>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    if let Some(messages) = body.messages {
        if messages.is_empty() {
            return Err(OmemError::Validation("messages array is empty".to_string()));
        }

        let mode = match body.mode.as_deref() {
            Some("raw") => IngestMode::Raw,
            _ => IngestMode::Smart,
        };

        let request = IngestRequest {
            messages: messages
                .into_iter()
                .map(|m| IngestMessage {
                    role: m.role,
                    content: m.content,
                })
                .collect(),
            tenant_id: auth.tenant_id.clone(),
            agent_id: body.agent_id.or(auth.agent_id),
            session_id: body.session_id,
            entity_context: body.entity_context,
            mode,
        };

        let session_store = state
            .store_manager
            .get_session_store(&auth.tenant_id)
            .await
            .map_err(|e| OmemError::Storage(format!("session store: {e}")))?;

        let ingest_pipeline =
            IngestPipeline::new(
                store,
                session_store,
                state.embed.clone(),
                state.llm.clone(),
                state.cluster_store.clone(),
                &state.config.admission_preset,
                state.config.admission_reject_threshold,
                state.config.admission_admit_threshold,
            ).await?.with_ingest_semaphore(state.ingest_semaphore.clone());

        let response = ingest_pipeline.ingest(request).await?;
        return Ok((StatusCode::ACCEPTED, Json(serde_json::json!(response))).into_response());
    }

    let content = body.content.ok_or_else(|| {
        OmemError::Validation("either 'messages' or 'content' required".to_string())
    })?;

    if content.is_empty() {
        return Err(OmemError::Validation("content cannot be empty".to_string()));
    }

    let mut memory = Memory::new(
        &content,
        Category::Preferences,
        MemoryType::Pinned,
        &auth.tenant_id,
    );
    memory.tags = body.tags.unwrap_or_default();
    memory.source = body.source;
    memory.agent_id = auth.agent_id.clone();
    if let Some(tier_str) = body.tier {
        memory.tier = tier_str
            .parse()
            .map_err(|e: String| OmemError::Validation(e))?;
    }
    if let Some(scope) = body.scope {
        memory.scope = scope;
    }
    if let Some(session_id) = body.session_id {
        memory.session_id = Some(session_id);
    }
    if let Some(ref agent_id) = body.agent_id {
        memory.agent_id = Some(agent_id.clone());
    }

    let visibility = body.visibility.unwrap_or_else(|| "global".to_string());
    memory.visibility = visibility.clone();
    if visibility == "private" {
        if let Some(ref agent_id) = body.agent_id {
            memory.owner_agent_id = agent_id.clone();
        } else if let Some(ref agent_id) = auth.agent_id {
            memory.owner_agent_id = agent_id.clone();
        }
    }

    let vectors = state
        .embed
        .embed(&[content])
        .await
        .map_err(|e| OmemError::Embedding(format!("failed to embed content: {e}")))?;
    let vector = vectors.into_iter().next();

    store.create(&memory, vector.as_deref()).await?;

    // Fire-and-forget: check auto-share rules for the newly created memory
    {
        let as_memory = memory.clone();
        let as_user = auth.tenant_id.clone();
        let as_agent = as_memory.agent_id.clone().unwrap_or_default();
        let as_space_store = state.space_store.clone();
        let as_store_mgr = state.store_manager.clone();
        tokio::spawn(async move {
            if let Err(e) = super::sharing::check_auto_share(
                &as_memory,
                &as_space_store,
                &as_store_mgr,
                &as_user,
                &as_agent,
            )
            .await
            {
                tracing::warn!(
                    memory_id = %as_memory.id,
                    error = %e,
                    "auto-share check failed (non-fatal)"
                );
            }
        });
    }

    Ok((StatusCode::CREATED, Json(serde_json::json!(memory))).into_response())
}

/// GET /v1/memories/search
pub async fn search_memories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponseDto>, OmemError> {
    if params.q.is_empty() {
        return Err(OmemError::Validation(
            "query parameter 'q' is required".to_string(),
        ));
    }

    let search_limit = params.limit.min(MAX_SEARCH_LIMIT);

    let vectors = state
        .embed
        .embed(std::slice::from_ref(&params.q))
        .await
        .map_err(|e| OmemError::Embedding(format!("failed to embed query: {e}")))?;
    let query_vector = vectors.into_iter().next();

    let spaces = state
        .space_store
        .list_spaces_for_user(&auth.tenant_id)
        .await?;

    let accessible_space_ids: Vec<String> = spaces.iter().map(|s| s.id.clone()).collect();

    if spaces.is_empty() {
        let store = state
            .store_manager
            .get_store(&personal_space_id(&auth.tenant_id))
            .await?;

        let request = SearchRequest {
            query: params.q,
            query_vector,
            tenant_id: auth.tenant_id,
            scope_filter: params.scope,
            limit: Some(search_limit),
            min_score: params.min_score,
            include_trace: params.include_trace,
            tags_filter: params
                .tags
                .as_ref()
                .map(|t| t.split(',').map(|s| s.trim().to_string()).collect()),
            source_filter: params.source.clone(),
            agent_id_filter: params.agent_id.clone(),
            accessible_spaces: accessible_space_ids.clone(),
        };

        let mut retrieval_pipeline = RetrievalPipeline::new(store.clone());
        if let Some(ref reranker) = state.reranker {
            retrieval_pipeline = retrieval_pipeline.with_reranker(reranker.clone());
        }
        let search_results = retrieval_pipeline.search(&request).await?;

        let mut results: Vec<SearchResultDto> = search_results
            .results
            .into_iter()
            .map(|r| SearchResultDto {
                memory: r.memory,
                score: r.score,
                stale_info: None,
            })
            .collect();

        if params.check_stale {
            for result in &mut results {
                result.stale_info =
                    check_stale_for_memory(&result.memory, &state.store_manager).await;
            }
        }

        let trace = build_trace(params.include_trace, &search_results.trace);

        // Fire-and-forget: increment access_count and evaluate tier for search results
        {
            let update_store = store;
            let memories_to_update: Vec<Memory> = results.iter().map(|r| r.memory.clone()).collect();
            tracing::debug!(count = memories_to_update.len(), "search_access_count_update_start");
            tokio::spawn(async move {
                for mut memory in memories_to_update {
                    let old_tier = memory.tier.clone();
                    let old_count = memory.access_count;
                    memory.access_count += 1;
                    memory.last_accessed_at = Some(chrono::Utc::now().to_rfc3339());
                    let new_tier = TierManager::with_defaults().evaluate_tier(&memory);
                    if new_tier != old_tier {
                        tracing::info!(memory_id = %memory.id, old_tier = %old_tier, new_tier = %new_tier, access_count = old_count + 1, "tier_promoted_via_search");
                        memory.append_tier_change(&old_tier.to_string(), &new_tier.to_string(), "access_via_search");
                    }
                    memory.tier = new_tier;
                    if let Err(e) = update_store.update(&memory, None).await {
                        tracing::warn!(memory_id = %memory.id, error = %e, "failed_to_update_access_count_after_search");
                    }
                }
            });
        }

        return Ok(Json(SearchResponseDto { results, trace }));
    }

    let target_spaces: Vec<_> = if let Some(ref space_param) = params.space {
        if space_param == "all" {
            spaces
        } else {
            let requested: Vec<&str> = space_param.split(',').map(|s| s.trim()).collect();
            spaces
                .into_iter()
                .filter(|s| requested.contains(&s.id.as_str()))
                .collect()
        }
    } else {
        spaces
    };

    let accessible = state
        .store_manager
        .get_accessible_stores(&auth.tenant_id, &target_spaces)
        .await?;

    // Parallel cross-space search via JoinSet
    let mut join_set = tokio::task::JoinSet::new();
    for acc in accessible {
        let query = params.q.clone();
        let query_vector = query_vector.clone();
        let tenant_id = auth.tenant_id.clone();
        let scope_filter = params.scope.clone();
        let limit = search_limit;
        let min_score = params.min_score;
        let tags_filter = params.tags.as_ref().map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>()
        });
        let source_filter = params.source.clone();
        let agent_id_filter = params.agent_id.clone();
        let store = acc.store.clone();
        let space_id = acc.space_id.clone();
        let weight = acc.weight;

        let accessible_spaces_clone = accessible_space_ids.clone();
        let reranker_clone = state.reranker.clone();
        join_set.spawn(async move {
            let request = SearchRequest {
                query,
                query_vector,
                tenant_id,
                scope_filter,
                limit: Some(limit),
                min_score,
                include_trace: false,
                tags_filter,
                source_filter,
                agent_id_filter,
                accessible_spaces: accessible_spaces_clone,
            };
            let mut pipeline = RetrievalPipeline::new(store);
            if let Some(reranker) = reranker_clone {
                pipeline = pipeline.with_reranker(reranker);
            }
            let result = pipeline.search(&request).await;
            (space_id, weight, result)
        });
    }

    let mut all_results: Vec<(Memory, f32, String)> = Vec::new();
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok((space_id, weight, Ok(search_results))) => {
                let max_score = search_results
                    .results
                    .iter()
                    .map(|r| r.score)
                    .fold(0.0_f32, f32::max);

                for r in search_results.results {
                    let normalized = if max_score > 0.0 {
                        r.score / max_score
                    } else {
                        0.0
                    };
                    let weighted = normalized * weight;
                    all_results.push((r.memory, weighted, space_id.clone()));
                }
            }
            Ok((space_id, _, Err(e))) => {
                tracing::warn!(space_id = %space_id, error = %e, "cross-space search failed for space, skipping");
            }
            Err(e) => {
                tracing::warn!(error = %e, "join error in cross-space search");
            }
        }
    }

    all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    all_results.truncate(search_limit);

    let mut results: Vec<SearchResultDto> = all_results
        .into_iter()
        .map(|(memory, score, _space_id)| SearchResultDto {
            memory,
            score,
            stale_info: None,
        })
        .collect();

    if params.check_stale {
        for result in &mut results {
            result.stale_info = check_stale_for_memory(&result.memory, &state.store_manager).await;
        }
    }

    // Fire-and-forget: increment access_count and evaluate tier for cross-space search results
    {
        let mgr = state.store_manager.clone();
        let memories_to_update: Vec<(String, Memory)> = results
            .iter()
            .map(|r| (r.memory.space_id.clone(), r.memory.clone()))
            .collect();
        tracing::debug!(count = memories_to_update.len(), query = %params.q, "cross_space_access_count_update_start");
        tokio::spawn(async move {
            for (space_id, mut memory) in memories_to_update {
                if let Ok(store) = mgr.get_store(&space_id).await {
                    let old_tier = memory.tier.clone();
                    let old_count = memory.access_count;
                    memory.access_count += 1;
                    memory.last_accessed_at = Some(chrono::Utc::now().to_rfc3339());
                    let new_tier = TierManager::with_defaults().evaluate_tier(&memory);
                    if new_tier != old_tier {
                        tracing::info!(memory_id = %memory.id, old_tier = %old_tier, new_tier = %new_tier, access_count = old_count + 1, space_id = %space_id, "tier_promoted_via_cross_space_search");
                        memory.append_tier_change(&old_tier.to_string(), &new_tier.to_string(), "access_via_cross_space_search");
                    }
                    memory.tier = new_tier;
                    if let Err(e) = store.update(&memory, None).await {
                        tracing::warn!(memory_id = %memory.id, error = %e, "failed_to_update_access_count_after_cross_space_search");
                    }
                }
            }
        });
    }

    Ok(Json(SearchResponseDto {
        results,
        trace: None,
    }))
}

fn build_trace(
    include: bool,
    trace: &crate::retrieve::trace::RetrievalTrace,
) -> Option<serde_json::Value> {
    if !include {
        return None;
    }
    Some(serde_json::json!({
        "stages": trace.stages.iter().map(|s| {
            serde_json::json!({
                "name": s.name,
                "input_count": s.input_count,
                "output_count": s.output_count,
                "duration_ms": s.duration_ms,
                "score_range": s.score_range,
            })
        }).collect::<Vec<_>>(),
        "total_duration_ms": trace.total_duration_ms,
        "final_count": trace.final_count,
    }))
}

pub(crate) async fn check_stale_for_memory(
    memory: &Memory,
    store_manager: &StoreManager,
) -> Option<StaleInfo> {
    let provenance = memory.provenance.as_ref()?;

    let source_store = store_manager
        .get_store(&provenance.shared_from_space)
        .await
        .ok()?;

    match source_store.get_by_id(&provenance.shared_from_memory).await {
        Ok(Some(source)) => {
            let source_ver = provenance.source_version.unwrap_or(0);
            let current_ver = source.version.unwrap_or(0);
            Some(StaleInfo {
                is_stale: source_ver < current_ver,
                source_version: provenance.source_version,
                current_source_version: source.version,
                source_deleted: false,
            })
        }
        Ok(None) => Some(StaleInfo {
            is_stale: true,
            source_version: provenance.source_version,
            current_source_version: None,
            source_deleted: true,
        }),
        Err(_) => None,
    }
}

/// GET /v1/memories/{id}
pub async fn get_memory(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
    Query(params): Query<GetMemoryQuery>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let mut memory = store
        .get_by_id(&id)
        .await?
        .ok_or_else(|| OmemError::NotFound(format!("memory {id}")))?;

    let old_tier = memory.tier.clone();
    let old_count = memory.access_count;
    if !params.skip_access {
        memory.access_count += 1;
        memory.last_accessed_at = Some(chrono::Utc::now().to_rfc3339());
        let new_tier = TierManager::with_defaults().evaluate_tier(&memory);
        if new_tier != old_tier {
            tracing::info!(memory_id = %memory.id, old_tier = %old_tier, new_tier = %new_tier, access_count = old_count + 1, "tier_promoted");
            memory.append_tier_change(&old_tier.to_string(), &new_tier.to_string(), "access_via_get");
        } else {
            tracing::debug!(memory_id = %memory.id, tier = %new_tier, access_count = old_count + 1, "access_count_incremented");
        }
        memory.tier = new_tier;
        store.update(&memory, None).await?;
    }

    let mut response = serde_json::to_value(&memory)
        .map_err(|e| OmemError::Internal(format!("serialize failed: {e}")))?;

    if params.check_stale {
        if let Some(stale_info) = check_stale_for_memory(&memory, &state.store_manager).await {
            response["stale_info"] = serde_json::to_value(&stale_info)
                .map_err(|e| OmemError::Internal(format!("serialize stale_info: {e}")))?;
        }
    }

    Ok(Json(response))
}

/// PUT /v1/memories/{id}
pub async fn update_memory(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
    Json(body): Json<UpdateMemoryBody>,
) -> Result<Json<Memory>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let mut memory = store
        .get_by_id(&id)
        .await?
        .ok_or_else(|| OmemError::NotFound(format!("memory {id}")))?;

    let mut need_reembed = false;

    if let Some(content) = body.content {
        if content.is_empty() {
            return Err(OmemError::Validation("content cannot be empty".to_string()));
        }
        memory.content = content.clone();
        memory.l2_content = content;
        need_reembed = true;
    }

    if let Some(tags) = body.tags {
        memory.tags = tags;
    }

    if let Some(state_str) = body.state {
        memory.state = state_str
            .parse()
            .map_err(|e: String| OmemError::Validation(e))?;
    }

    if let Some(tier_str) = body.tier {
        memory.tier = tier_str
            .parse()
            .map_err(|e: String| OmemError::Validation(e))?;
    }

    if let Some(th) = body.tier_history {
        memory.tier_history = if th.is_empty() { None } else { Some(th) };
    }

    memory.updated_at = chrono::Utc::now().to_rfc3339();

    let vector = if need_reembed {
        let vectors = state
            .embed
            .embed(&[memory.content.clone()])
            .await
            .map_err(|e| OmemError::Embedding(format!("failed to embed content: {e}")))?;
        vectors.into_iter().next()
    } else {
        None
    };

    store.update(&memory, vector.as_deref()).await?;

    Ok(Json(memory))
}

/// DELETE /v1/memories/{id}
pub async fn delete_memory(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    store
        .get_by_id(&id)
        .await?
        .ok_or_else(|| OmemError::NotFound(format!("memory {id}")))?;

    store.hard_delete(&id).await?;

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

/// GET /v1/memories
pub async fn list_memories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<ListQuery>,
) -> Result<Json<ListResponseDto>, OmemError> {
    let space_id = personal_space_id(&auth.tenant_id);
    let store = state
        .store_manager
        .get_store(&space_id)
        .await?;

    let filter = ListFilter {
        q: params.q,
        category: params.category,
        tier: params.tier,
        tags: params
            .tags
            .map(|t| t.split(',').map(|s| s.trim().to_string()).collect()),
        memory_type: params.memory_type,
        state: params.state,
        visibility: params.visibility,
        sort: params.sort,
        order: params.order,
    };

    let total_count = store.count_filtered(&filter).await?;
    let mut memories = store
        .list_filtered(&filter, params.limit, params.offset)
        .await?;

    let has_vault = state
        .space_store
        .get_vault_password(&space_id)
        .await
        .ok()
        .flatten()
        .is_some();

    if has_vault {
        for m in &mut memories {
            if m.scope == "private" {
                m.content = "🔒 [Vault Locked]".to_string();
                m.l1_overview = "🔒 [Vault Locked]".to_string();
                m.l2_content = "🔒 [Vault Locked]".to_string();
            }
        }
    }

    Ok(Json(ListResponseDto {
        memories,
        total_count,
        limit: params.limit,
        offset: params.offset,
    }))
}

// ── Batch Get ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BatchGetRequest {
    pub ids: Vec<String>,
}

pub async fn batch_get_memories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<BatchGetRequest>,
) -> Result<Json<Vec<Memory>>, OmemError> {
    if body.ids.is_empty() {
        return Ok(Json(vec![]));
    }
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let memories = store.get_memories_by_ids(&body.ids).await?;
    Ok(Json(memories))
}

// ── Batch Visibility ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BatchVisibilityRequest {
    pub memory_ids: Vec<String>,
    pub visibility: Option<String>,
    pub agent_id: Option<String>,
    pub scope: Option<String>,
}

fn validate_scope(scope: &str) -> Result<(), OmemError> {
    match scope {
        "public" | "private" | "global" | "team" | "org" => Ok(()),
        _ => Err(OmemError::Validation(
            "scope must be 'public', 'private', 'global', 'team', or 'org'".to_string(),
        )),
    }
}

fn validate_visibility(visibility: &str) -> Result<(), OmemError> {
    if visibility == "private" || visibility == "global" || visibility.starts_with("shared:") {
        Ok(())
    } else {
        Err(OmemError::Validation(
            "visibility must be 'private', 'global', or 'shared:*'".to_string(),
        ))
    }
}

pub async fn batch_update_visibility(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<BatchVisibilityRequest>,
) -> Result<Json<serde_json::Value>, OmemError> {
    if body.memory_ids.is_empty() {
        return Ok(Json(serde_json::json!({ "updated": 0 })));
    }

    if let Some(ref visibility) = body.visibility {
        validate_visibility(visibility)?;
    }
    if let Some(ref scope) = body.scope {
        validate_scope(scope)?;
    }
    if body.visibility.is_none() && body.scope.is_none() {
        return Err(OmemError::Validation(
            "at least one of 'visibility' or 'scope' must be provided".to_string(),
        ));
    }

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let memories = store.get_memories_by_ids(&body.memory_ids).await?;
    let mut updated = 0usize;

    for mut memory in memories {
        if let Some(ref visibility) = body.visibility {
            memory.visibility = visibility.clone();
        }
        if let Some(ref scope) = body.scope {
            memory.scope = scope.clone();
            memory.visibility = if scope == "private" {
                "private".to_string()
            } else {
                "global".to_string()
            };
            if scope == "private" {
                if !memory.tags.contains(&"私密".to_string()) {
                    memory.tags.push("私密".to_string());
                }
            } else {
                memory.tags.retain(|t| t != "私密");
            }
        }
        if memory.visibility == "private" {
            if let Some(ref agent_id) = body.agent_id {
                memory.owner_agent_id = agent_id.clone();
            } else if let Some(ref agent_id) = auth.agent_id {
                memory.owner_agent_id = agent_id.clone();
            }
        }
        memory.updated_at = chrono::Utc::now().to_rfc3339();
        store.update(&memory, None).await?;
        updated += 1;
    }

    Ok(Json(serde_json::json!({ "updated": updated })))
}

// ── Batch Delete ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BatchDeleteRequest {
    pub memory_ids: Option<Vec<String>>,
    pub filter: Option<BatchDeleteFilter>,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Deserialize)]
pub struct BatchDeleteFilter {
    pub source: Option<String>,
    pub tags: Option<Vec<String>>,
    pub category: Option<String>,
    pub memory_type: Option<String>,
    pub state: Option<String>,
    pub before: Option<String>,
}

fn build_batch_delete_where(filter: &BatchDeleteFilter) -> String {
    let mut conditions = Vec::new();

    if let Some(ref source) = filter.source {
        conditions.push(format!("source LIKE '{}%'", source.replace('\'', "''")));
    }
    if let Some(ref tags) = filter.tags {
        for tag in tags {
            let escaped = tag.replace('\'', "''");
            conditions.push(format!("(tags LIKE '%\"{}\"%')", escaped));
        }
    }
    if let Some(ref cat) = filter.category {
        conditions.push(format!("category = '{}'", cat.replace('\'', "''")));
    }
    if let Some(ref mt) = filter.memory_type {
        conditions.push(format!("memory_type = '{}'", mt.replace('\'', "''")));
    }
    if let Some(ref state) = filter.state {
        conditions.push(format!("state = '{}'", state.replace('\'', "''")));
    }
    if let Some(ref before) = filter.before {
        conditions.push(format!("created_at < '{}'", before.replace('\'', "''")));
    }

    if conditions.is_empty() {
        "true".to_string()
    } else {
        conditions.join(" AND ")
    }
}

pub async fn batch_delete(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<BatchDeleteRequest>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    if let Some(ids) = body.memory_ids {
        let deleted = store.batch_hard_delete_by_ids(&ids).await?;
        return Ok(Json(serde_json::json!({
            "deleted": deleted,
            "mode": "ids"
        })));
    }

    if let Some(ref filter) = body.filter {
        let where_clause = build_batch_delete_where(filter);

        if !body.confirm {
            let count = store.count_by_filter(&where_clause).await?;
            return Ok(Json(serde_json::json!({
                "would_delete": count
            })));
        }

        let deleted = store.batch_hard_delete(&where_clause).await?;
        return Ok(Json(serde_json::json!({
            "deleted": deleted,
            "mode": "filter"
        })));
    }

    Err(OmemError::Validation(
        "provide either memory_ids or filter".to_string(),
    ))
}

pub async fn delete_all_memories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, OmemError> {
    let confirm = headers.get("X-Confirm").and_then(|v| v.to_str().ok());
    if confirm != Some("delete-all") {
        return Err(OmemError::Validation(
            "DELETE /v1/memories/all requires X-Confirm: delete-all header".to_string(),
        ));
    }

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let count = store.delete_all().await?;

    let session_store = state
        .store_manager
        .get_session_store(&auth.tenant_id)
        .await
        .map_err(|e| OmemError::Storage(format!("session store: {e}")))?;
    let sessions_cleared = session_store.delete_all().await?;

    Ok(Json(serde_json::json!({
        "deleted": count,
        "sessions_cleared": sessions_cleared
    })))
}

// ── Tier Changes ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TierChangesQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierChangeItem {
    pub memory_id: String,
    pub memory_title: String,
    pub from: String,
    pub to: String,
    pub reason: String,
    pub at: String,
    pub access_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierChangesResponse {
    pub changes: Vec<TierChangeItem>,
    pub total_count: usize,
    pub limit: usize,
    pub offset: usize,
}

pub async fn get_tier_changes(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<TierChangesQuery>,
) -> Result<Json<TierChangesResponse>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let filter = ListFilter {
        q: None,
        category: None,
        tier: None,
        tags: None,
        memory_type: None,
        state: Some("active".to_string()),
        visibility: None,
        sort: String::new(),
        order: String::new(),
    };
    let memories = store.list_filtered(&filter, 2000, 0).await?;

    let tier_order = |t: &str| -> i32 {
        match t {
            "peripheral" => 0,
            "working" => 1,
            "core" => 2,
            _ => 0,
        }
    };

    let mut all_changes: Vec<TierChangeItem> = Vec::new();

    for mem in &memories {
        if let Some(ref hist) = mem.tier_history {
            if hist.is_empty() {
                continue;
            }
            let events: Vec<serde_json::Value> = match serde_json::from_str(hist) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let title = if mem.l0_abstract.is_empty() {
                mem.content.chars().take(40).collect::<String>()
            } else {
                mem.l0_abstract.clone()
            };
            for ev in events {
                let from = ev["from"].as_str().unwrap_or("").to_string();
                let to = ev["to"].as_str().unwrap_or("").to_string();
                let reason = ev["reason"].as_str().unwrap_or("").to_string();
                let at = ev["at"].as_str().unwrap_or("").to_string();
                let access_count = ev["access_count"].as_u64().unwrap_or(0) as u32;

                if let Some(ref f) = params.filter {
                    let from_rank = tier_order(&from);
                    let to_rank = tier_order(&to);
                    match f.as_str() {
                        "promote" if from_rank >= to_rank => continue,
                        "demote" if from_rank <= to_rank => continue,
                        _ => {}
                    }
                }

                if let Some(ref q) = params.search {
                    let ql = q.to_lowercase();
                    let haystack = format!("{} {} {} {} {}", mem.id, title, from, to, reason).to_lowercase();
                    if !haystack.contains(&ql) {
                        continue;
                    }
                }

                all_changes.push(TierChangeItem {
                    memory_id: mem.id.clone(),
                    memory_title: title.clone(),
                    from,
                    to,
                    reason,
                    at,
                    access_count,
                });
            }
        }
    }

    all_changes.sort_by(|a, b| b.at.cmp(&a.at));

    let total_count = all_changes.len();
    let paged: Vec<TierChangeItem> = all_changes
        .into_iter()
        .skip(params.offset)
        .take(params.limit)
        .collect();

    Ok(Json(TierChangesResponse {
        changes: paged,
        total_count,
        limit: params.limit,
        offset: params.offset,
    }))
}

#[derive(Deserialize)]
pub struct DeleteTierHistoryBody {
    pub memory_id: String,
    pub from: String,
    pub to: String,
    pub at: String,
    pub reason: String,
}

pub async fn delete_tier_history_entry(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<DeleteTierHistoryBody>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let mut mem = store.get_by_id(&body.memory_id).await?
        .ok_or_else(|| OmemError::NotFound("Memory not found".to_string()))?;

    let tier_history = mem.tier_history.take().unwrap_or_default();
    if tier_history.is_empty() {
        return Ok(Json(serde_json::json!({ "deleted": false, "reason": "no history" })));
    }

    let mut history: Vec<serde_json::Value> = serde_json::from_str(&tier_history)
        .unwrap_or_default();

    let before = history.len();
    history.retain(|e| {
        !(e["from"].as_str().unwrap_or("") == body.from
            && e["to"].as_str().unwrap_or("") == body.to
            && e["at"].as_str().unwrap_or("") == body.at
            && e["reason"].as_str().unwrap_or("") == body.reason)
    });
    let deleted = history.len() < before;

    mem.tier_history = if history.is_empty() { None } else { Some(serde_json::to_string(&history).unwrap_or_default()) };
    let vector = store.get_vector_by_id(&body.memory_id).await?.or(None);
    store.update(&mem, vector.as_deref()).await?;

    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

#[derive(Serialize)]
pub struct ReEmbedResponseDto {
    pub re_embedded: usize,
    pub skipped_nonzero: usize,
    pub errors: usize,
}

pub async fn reembed_memories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<ReEmbedResponseDto>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let memories = store.list_all_active().await?;
    let total = memories.len();

    // Spawn background task for batched re-embedding
    let embed = state.embed.clone();
    let import_semaphore = state.import_semaphore.clone();
    let _permit = import_semaphore.acquire().await.map_err(|_| {
        OmemError::Internal("import semaphore closed".to_string())
    })?;

    tokio::spawn(async move {
        let batch_size = 20usize;
        let mut re_embedded = 0usize;
        let mut errors = 0usize;

        for chunk in memories.chunks(batch_size) {
            let texts: Vec<String> = chunk
                .iter()
                .map(|m| {
                    if !m.l0_abstract.is_empty() {
                        m.l0_abstract.clone()
                    } else {
                        m.content.clone()
                    }
                })
                .collect();

            match embed.embed(&texts).await {
                Ok(vectors) => {
                    for (memory, vector) in chunk.iter().zip(vectors.into_iter()) {
                        match store.update(memory, Some(&vector)).await {
                            Ok(_) => re_embedded += 1,
                            Err(e) => {
                                tracing::warn!(id = %memory.id, error = %e, "re-embed update failed");
                                errors += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "re-embed batch failed, skipping {} memories", chunk.len());
                    errors += chunk.len();
                }
            }
        }

        tracing::info!(total, re_embedded, errors, "re-embed completed");
    });

    Ok(Json(ReEmbedResponseDto {
        re_embedded: 0,
        skipped_nonzero: 0,
        errors: 0,
    }))
}

// ── Session Ingest ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SessionIngestBody {
    pub messages: Vec<MessageDto>,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_title: Option<String>,
    pub project_name: Option<String>,
}

#[derive(Deserialize)]
struct SessionTopicSummary {
    topic: String,
    summary: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_scope")]
    scope: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    memory_type: Option<String>,
}

fn default_scope() -> String {
    "public".to_string()
}

pub async fn session_ingest(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<SessionIngestBody>,
) -> Result<impl IntoResponse, OmemError> {
    if body.messages.is_empty() {
        return Err(OmemError::Validation(
            "messages array is empty".to_string(),
        ));
    }

    // Validate conversation content synchronously before going async
    const MAX_MESSAGES: usize = 40;
    let messages: Vec<MessageDto> = if body.messages.len() > MAX_MESSAGES {
        body.messages[body.messages.len() - MAX_MESSAGES..].to_vec()
    } else {
        body.messages.clone()
    };

    const MAX_CONVERSATION_CHARS: usize = 30_000;
    let conversation = format_conversation_truncated(&messages, MAX_CONVERSATION_CHARS);
    if conversation.is_empty() {
        return Err(OmemError::Validation(
            "no valid conversation content after cleaning".to_string(),
        ));
    }

    let tenant_id = auth.tenant_id.clone();
    let agent_id = body.agent_id.or(auth.agent_id.clone());
    let session_id = body.session_id.clone();
    let session_key = body.session_id.as_deref().unwrap_or("default").to_string();
    let response_session_id = session_id.clone();

    // Fire-and-forget: process in background, return 202 immediately
    tokio::spawn(async move {
        // Acquire per-session lock inside background task
        let lock_arc = state
            .session_locks
            .entry(session_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));
        let _session_guard = lock_arc.lock().await;
        let cluster_store = state.cluster_store.clone();
        let llm_for_cluster = Some(state.llm.clone());
        let store = match state
            .store_manager
            .get_store(&personal_space_id(&tenant_id))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "session_ingest_bg: failed to get store");
                return;
            }
        };

        let cluster_assigner = crate::cluster::assigner::ClusterAssigner::new(
            cluster_store.clone(),
            state.embed.clone(),
        )
        .with_llm(state.llm.clone())
        .with_lance_store(Some(store.clone()));

        // 获取同session的EMOTIONAL记忆（用于追加到已有记忆）
        let mut existing_emotional = fetch_session_emotional_memory(
            &store,
            session_id.as_deref(),
        ).await;

        // 获取同session的WORK记忆（用于追加到已有记忆）
        let mut existing_work_memory = fetch_session_work_memory(
            &store,
            session_id.as_deref(),
        ).await;

        // 独立提取模式：只处理当前conversation，不传旧summaries
        let (system_prompt, user_prompt) = crate::ingest::prompts::build_session_extract_prompt(
            &conversation,
        );

        let topics: Vec<SessionTopicSummary> = match crate::llm::complete_json(
            state.llm.as_ref(),
            &system_prompt,
            &user_prompt,
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = %e, "session_ingest_bg: LLM extract failed");
                return;
            }
        };

        if topics.is_empty() {
            tracing::info!(tenant = %tenant_id, "session_ingest_bg: LLM returned no topics");
            return;
        }

        let topic_texts: Vec<String> = topics.iter().map(|t| t.topic.clone()).collect();
        let vectors = match state.embed.embed(&topic_texts).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, "session_ingest_bg: failed to embed topics");
                return;
            }
        };

        let mut stored = 0usize;
        let mut created_memories: Vec<(Memory, Option<Vec<f32>>)> = Vec::new();

        for (i, topic) in topics.iter().enumerate() {
            let memory_type = topic.memory_type.as_deref().unwrap_or_else(|| {
                // 兼容旧prompt：根据scope和category推断
                if topic.scope == "private" {
                    "EMOTIONAL"
                } else if topic.category.as_deref() == Some("preferences") {
                    "PREFERENCE"
                } else {
                    "WORK"
                }
            });

            let category: Category = topic.category.as_deref().and_then(|c| {
                let normalized = match c.to_lowercase().as_str() {
                    "experience" | "experiences" | "activity" | "activities" => "events",
                    "knowledge" | "skill" | "skills" | "ability" | "abilities" => "patterns",
                    _ => c,
                };
                normalized.parse().ok()
            }).unwrap_or_else(|| {
                if topic.scope == "private" {
                    Category::Profile
                } else {
                    Category::Events
                }
            });

            let l1_overview = {
                let s = &topic.summary;
                if s.chars().count() <= 150 { s.clone() } else {
                    let truncated: String = s.chars().take(147).collect();
                    format!("{}...", truncated)
                }
            };
            let l2_content = {
                let s = &topic.summary;
                if s.chars().count() <= 500 { s.clone() } else {
                    let truncated: String = s.chars().take(497).collect();
                    format!("{}...", truncated)
                }
            };
            let tags = if topic.tags.is_empty() {
                vec!["session_compress".to_string()]
            } else {
                let mut t = topic.tags.clone();
                t.push("session_compress".to_string());
                if memory_type == "PREFERENCE" {
                    t.push("preference_extract".to_string());
                }
                t
            };

            let mut memory = Memory::new(
                &topic.summary,
                category,
                MemoryType::Pinned,
                &tenant_id,
            );
            memory.l0_abstract = topic.topic.clone();
            memory.l1_overview = l1_overview;
            memory.l2_content = l2_content;
            memory.source = Some("session_compress".to_string());
            memory.session_id = session_id.clone();
            memory.agent_id = agent_id.clone();
            memory.tags = tags;
            if topic.scope == "private" {
                memory.scope = "private".to_string();
                memory.visibility = "private".to_string();
            }

            // EMOTIONAL类追加逻辑：同session的private记忆追加到已有记忆
            if topic.scope == "private" {
                if let Some(existing) = existing_emotional.clone() {
                    let new_content = format!(
                        "{}\n\n---\n\n## {}\n{}",
                        existing.content,
                        topic.topic,
                        topic.summary
                    );

                    if new_content.chars().count() <= 3000 {
                        let mut updated = existing;
                        updated.content = new_content.clone();
                        let abstract_text: String = new_content.chars().take(200).collect();
                        updated.l0_abstract = abstract_text;
                        updated.l1_overview = if new_content.chars().count() <= 150 {
                            new_content.clone()
                        } else {
                            format!("{}...", new_content.chars().take(147).collect::<String>())
                        };
                        updated.l2_content = if new_content.chars().count() <= 500 {
                            new_content.clone()
                        } else {
                            format!("{}...", new_content.chars().take(497).collect::<String>())
                        };
                        for tag in &topic.tags {
                            if !updated.tags.contains(tag) {
                                updated.tags.push(tag.clone());
                            }
                        }

                        if let Err(e) = store.update(&updated, None).await {
                            tracing::warn!(error = %e, "session_ingest: failed to append to existing emotional memory");
                        } else {
                            tracing::info!(memory_id = %updated.id, "session_ingest: appended to existing emotional memory");
                            existing_emotional = Some(updated);
                            continue;
                        }
                    }
                    tracing::info!("session_ingest: emotional memory exceeded limit or update failed, creating new");
                }
            }

            // WORK类追加逻辑：同session的WORK记忆追加到已有记忆
            if memory_type == "WORK" {
                if let Some(existing_work) = existing_work_memory.clone() {
                    let new_content = format!(
                        "{}\n\n---\n\n## {}\n{}",
                        existing_work.content,
                        topic.topic,
                        topic.summary
                    );

                    if new_content.chars().count() <= 3000 {
                        let mut updated = existing_work;
                        updated.content = new_content.clone();
                        let abstract_text: String = new_content.chars().take(200).collect();
                        updated.l0_abstract = abstract_text;
                        updated.l1_overview = if new_content.chars().count() <= 150 {
                            new_content.clone()
                        } else {
                            format!("{}...", new_content.chars().take(147).collect::<String>())
                        };
                        updated.l2_content = if new_content.chars().count() <= 500 {
                            new_content.clone()
                        } else {
                            format!("{}...", new_content.chars().take(497).collect::<String>())
                        };
                        for tag in &topic.tags {
                            if !updated.tags.contains(tag) {
                                updated.tags.push(tag.clone());
                            }
                        }

                        if let Err(e) = store.update(&updated, None).await {
                            tracing::warn!(error = %e, "session_ingest: failed to append to existing WORK memory");
                        } else {
                            tracing::info!(memory_id = %updated.id, "session_ingest: appended to existing WORK memory");
                            existing_work_memory = Some(updated);
                            continue;
                        }
                    }
                    tracing::info!("session_ingest: WORK memory exceeded limit, creating new");
                }
            }

            let vector = vectors.get(i).cloned();
            if let Err(e) = store.create(&memory, vector.as_deref()).await {
                tracing::error!(error = %e, "session_ingest_bg: create failed");
                return;
            }
            stored += 1;
            created_memories.push((memory.clone(), vector.clone()));
        }

        // ── Cluster语义归簇：逐条匹配已有cluster ──
        if !created_memories.is_empty() {
            let cluster_manager = crate::cluster::manager::ClusterManager::new(
                cluster_store.clone(),
                llm_for_cluster.clone(),
            );

            for (mem, vector) in &created_memories {
                match cluster_assigner.assign(mem).await {
                    Ok(result) => {
                        match result.action {
                            crate::cluster::assigner::AssignAction::AutoAssign => {
                                if let Some(ref cid) = result.cluster_id {
                                    match cluster_manager.assign_to_cluster(&mem.id, cid, store.clone()).await {
                                        Ok(_) => {
                                            tracing::info!(memory_id = %mem.id, cluster_id = %cid, "session_ingest: assigned to existing cluster");
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, memory_id = %mem.id, "session_ingest: failed to assign to cluster");
                                        }
                                    }
                                }
                            }
                            crate::cluster::assigner::AssignAction::CreateNew => {
                                // 用已有的vector创建cluster（避免重复embed）
                                if let Some(vec) = vector {
                                    match cluster_manager.create_cluster(mem, vec, mem.tags.clone()).await {
                                        Ok(cluster) => {
                                            if let Err(e) = cluster_manager.assign_to_cluster(&mem.id, &cluster.id, store.clone()).await {
                                                tracing::warn!(error = %e, memory_id = %mem.id, "session_ingest: failed to link to new cluster");
                                            } else {
                                                tracing::info!(memory_id = %mem.id, cluster_id = %cluster.id, "session_ingest: created new cluster");
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, memory_id = %mem.id, "session_ingest: failed to create cluster");
                                        }
                                    }
                                } else {
                                    tracing::warn!(memory_id = %mem.id, "session_ingest: no vector available for cluster creation");
                                }
                            }
                            crate::cluster::assigner::AssignAction::LlmJudge => {
                                // LLM裁决：有cluster_id则归入，无则创建新cluster
                                if let Some(ref cid) = result.cluster_id {
                                    match cluster_manager.assign_to_cluster(&mem.id, cid, store.clone()).await {
                                        Ok(_) => {
                                            tracing::info!(memory_id = %mem.id, cluster_id = %cid, "session_ingest: LLM-judged assignment to existing cluster");
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, memory_id = %mem.id, "session_ingest: failed LLM-judged assignment");
                                        }
                                    }
                                } else if let Some(vec) = vector {
                                    match cluster_manager.create_cluster(mem, vec, mem.tags.clone()).await {
                                        Ok(cluster) => {
                                            if let Err(e) = cluster_manager.assign_to_cluster(&mem.id, &cluster.id, store.clone()).await {
                                                tracing::warn!(error = %e, memory_id = %mem.id, "session_ingest: failed to link to LLM-judged new cluster");
                                            } else {
                                                tracing::info!(memory_id = %mem.id, cluster_id = %cluster.id, "session_ingest: LLM-judged new cluster created");
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, memory_id = %mem.id, "session_ingest: LLM-judged cluster creation failed");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, memory_id = %mem.id, "session_ingest: cluster assignment failed");
                    }
                }
            }
        }

        tracing::info!(
            stored = stored,
            tenant = %tenant_id,
            "session_ingest: created independent memories"
        );

        // ── Post-ingest compact: prevent version bloat from accumulating ──
        // Each ingest creates/updates multiple memories → LanceDB versions pile up fast.
        // Compact here keeps versions low between scheduler cycles.
        if stored > 0 {
            if let Err(e) = store.optimize().await {
                tracing::warn!(error = %e, "session_ingest: post-ingest optimize failed");
            } else {
                tracing::info!(tenant = %tenant_id, "session_ingest: post-ingest optimize completed");
            }
        }
    });

    Ok((StatusCode::ACCEPTED, Json(serde_json::json!({
        "status": "accepted",
        "session_id": response_session_id,
    })))
    .into_response())
}

fn format_conversation(messages: &[MessageDto]) -> String {
    let mut formatted = String::with_capacity(4096);

    for msg in messages {
        let role = match msg.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            _ => continue,
        };

        let content = clean_message_content(&msg.content);
        if content.is_empty() {
            continue;
        }

        formatted.push_str(&format!("[{}]: {}\n\n", role, content));
    }

    formatted
}

/// Format conversation with a total character budget.
/// If total exceeds `max_chars`, truncate each message proportionally from the front
/// (keeping the most recent messages intact, truncating older ones).
fn format_conversation_truncated(messages: &[MessageDto], max_chars: usize) -> String {
    let formatted = format_conversation(messages);
    if formatted.len() <= max_chars {
        return formatted;
    }

    // Keep only the tail that fits within budget
    let truncated: String = formatted
        .chars()
        .skip(formatted.chars().count().saturating_sub(max_chars))
        .collect();

    // Drop partial first line (incomplete message)
    if let Some(pos) = truncated.find("\n[") {
        truncated[pos + 1..].to_string()
    } else {
        truncated
    }
}

fn clean_message_content(content: &str) -> String {
    let mut cleaned = content.to_string();

    // Remove XML-like system tags and their content
    let xml_patterns = [
        "<system-reminder>",
        "<auto-slash-command>",
        "<thinking>",
        "< omoc:",
        "<analysis>",
    ];
    for pattern in xml_patterns {
        while let Some(start) = cleaned.find(pattern) {
            let tag_name_end = cleaned[start..]
                .find(|c: char| c == ' ' || c == '>')
                .map(|i| start + i)
                .unwrap_or(cleaned.len());
            let tag_name = &cleaned[start..tag_name_end];
            let close_tag = format!("</{}>", &tag_name[1..]);

            if let Some(end) = cleaned.find(&close_tag) {
                cleaned = format!(
                    "{}{}",
                    &cleaned[..start],
                    &cleaned[end + close_tag.len()..]
                );
            } else {
                let end = cleaned[start..]
                    .find('\n')
                    .map(|i| start + i)
                    .unwrap_or(cleaned.len());
                cleaned = format!("{}{}", &cleaned[..start], &cleaned[end..]);
            }
        }
    }

    // Remove noise patterns
    let noise_patterns = [
        "[Compressed conversation section]",
        "[search-mode]",
        "[analyze-mode]",
        "MANDATORY delegate_task params",
        "[SYSTEM DIRECTIVE",
        "OH-MY-OPENCODE",
        "Incomplete tasks",
        "OMO_INTERNAL_INITIATOR",
    ];
    for pattern in noise_patterns {
        cleaned = cleaned.replace(pattern, "");
    }

    // Filter out lines starting with system prefixes
    let lines: Vec<&str> = cleaned.lines().filter(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() { return false; }
        if trimmed.starts_with("<dcp") { return false; }
        if trimmed.starts_with("</dcp") { return false; }
        if trimmed.starts_with("<dcf") { return false; }
        if trimmed.starts_with("</dcf") { return false; }
        if trimmed.starts_with("<dcp_message") { return false; }
        if trimmed.starts_with("</dcp_message") { return false; }
        if trimmed.starts_with("<system-reminder") { return false; }
        if trimmed.starts_with("</system-reminder") { return false; }
        if trimmed.starts_with("<auto-slash-command") { return false; }
        if trimmed.starts_with("</auto-slash-command") { return false; }
        true
    }).collect();

    cleaned = lines.join("\n");
    cleaned.trim().to_string()
}



/// Fetch the most recent private (EMOTIONAL) session_compress memory for a given session.
/// Used to append new emotional topics to the same memory within a session.
async fn fetch_session_emotional_memory(
    store: &crate::store::lancedb::LanceStore,
    session_id: Option<&str>,
) -> Option<Memory> {
    let Some(sid) = session_id else { return None; };

    let filter = ListFilter {
        tags: Some(vec!["session_compress".to_string()]),
        ..Default::default()
    };

    match store.list_filtered(&filter, 20, 0).await {
        Ok(memories) => memories
            .into_iter()
            .filter(|m| m.session_id.as_deref() == Some(sid) && m.scope == "private")
            .max_by_key(|m| m.updated_at.clone()),
        Err(_) => None,
    }
}

/// Fetch the most recent public (WORK) session_compress memory for a given session.
/// Used to append new work topics to the same memory within a session.
async fn fetch_session_work_memory(
    store: &crate::store::lancedb::LanceStore,
    session_id: Option<&str>,
) -> Option<Memory> {
    let Some(sid) = session_id else { return None; };

    let filter = ListFilter {
        tags: Some(vec!["session_compress".to_string()]),
        ..Default::default()
    };

    match store.list_filtered(&filter, 20, 0).await {
        Ok(memories) => memories
            .into_iter()
            .filter(|m| m.session_id.as_deref() == Some(sid) && m.scope != "private")
            .max_by_key(|m| m.updated_at.clone()),
        Err(_) => None,
    }
}

pub async fn optimize_memories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    store.optimize().await?;

    Ok(Json(serde_json::json!({"status": "ok"})))
}
