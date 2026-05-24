use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::{personal_space_id, AppState};
use crate::domain::category::Category;
use crate::domain::error::OmemError;
use crate::domain::memory::{sanitize_project_path, Memory};
use crate::domain::tenant::AuthInfo;
use crate::domain::types::MemoryType;
use crate::ingest::refine_service::{collect_chain_memories, refine_and_replace};
use crate::ingest::types::{IngestMessage, IngestMode, IngestRequest};
use crate::ingest::IngestPipeline;

use crate::lifecycle::decay::DecayEngine;
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
    pub project_name: Option<String>,
    pub project_path: Option<String>,

    // Direct single memory creation
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub tier: Option<String>,
    pub scope: Option<String>,
    pub visibility: Option<String>,
    pub category: Option<String>,
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
    #[serde(default)]
    pub project_path: Option<String>,
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
    pub project_path: Option<String>,
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
    pub category: Option<String>,
    pub state: Option<String>,
    pub tier: Option<String>,
    pub tier_history: Option<String>,
    pub session_id: Option<String>,
    pub project_path: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResultDto {
    pub memory: Memory,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_info: Option<StaleInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_relevance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_reasoning: Option<String>,
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

    let project_path = match body.project_path.as_deref() {
        Some(pp) if !pp.is_empty() => Some(sanitize_project_path(pp).map_err(|e| {
            OmemError::Validation(format!("invalid project_path: {e}"))
        })?),
        _ => None,
    };

    if let Some(messages) = body.messages {
        if messages.is_empty() {
            return Err(OmemError::Validation("messages array is empty".to_string()));
        }

        let mode = match body.mode.as_deref() {
            Some("raw") => IngestMode::Raw,
            _ => IngestMode::Smart,
        };

        let project_name = body.project_name.as_deref().map(|name| {
            name.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').take(32).collect::<String>()
        }).filter(|s| !s.is_empty());

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
            project_name,
            project_path: project_path.clone(),
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
                &state.config.admission_preset,
                state.config.admission_reject_threshold,
                state.config.admission_admit_threshold,
                state.category_registry.clone(),
                auth.tenant_id.clone(),
            ).await?.with_ingest_semaphore(state.ingest_semaphore.clone())
             .with_induction_engine(state.induction_engine.clone());

        let response = ingest_pipeline.ingest(request).await?;
        return Ok((StatusCode::ACCEPTED, Json(serde_json::json!(response))).into_response());
    }

    let content = body.content.ok_or_else(|| {
        OmemError::Validation("either 'messages' or 'content' required".to_string())
    })?;

    if content.is_empty() {
        return Err(OmemError::Validation("content cannot be empty".to_string()));
    }

    let category = if let Some(cat_str) = body.category {
        cat_str.parse::<Category>().unwrap()
    } else {
        Category::new("cases")
    };
    let mut memory = Memory::new(
        &content,
        category,
        MemoryType::Pinned,
        &auth.tenant_id,
    );
    memory.tags = body.tags.unwrap_or_default();
    memory.source = body.source;
    memory.agent_id = auth.agent_id.clone();
    memory.project_path = project_path;
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
        // Private memories are global — never bound to a project_path
        memory.project_path = None;
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

    // Sanitize project_path to prevent path traversal and SQL injection
    let sanitized_project_path = match params.project_path.as_deref() {
        Some(pp) if !pp.is_empty() => Some(sanitize_project_path(pp).map_err(|e| {
            OmemError::Validation(format!("invalid project_path: {e}"))
        })?),
        _ => None,
    };

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
            conversation_context: None,
            project_path_filter: sanitized_project_path.clone(),
        };

        let mut retrieval_pipeline = RetrievalPipeline::new(store.clone())
            .with_decay_config(state.config.decay_config());
        if let Some(ref reranker) = state.reranker {
            retrieval_pipeline = retrieval_pipeline.with_reranker(reranker.clone());
        }
        let search_results = retrieval_pipeline.search(&request, None).await?;

        let mut results: Vec<SearchResultDto> = search_results
            .results
            .into_iter()
            .map(|r| SearchResultDto {
                memory: r.memory,
                score: r.score,
                stale_info: None,
                refine_relevance: r.refine_relevance,
                refine_reasoning: r.refine_reasoning,
            })
            .collect();

        if params.check_stale {
            for result in &mut results {
                result.stale_info =
                    check_stale_for_memory(&result.memory, &state.store_manager).await;
            }
        }

        let trace = build_trace(params.include_trace, &search_results.trace);

        // Fire-and-forget: batch increment access_count for search results (single LanceDB version)
        {
            let update_store = store;
            let result_ids: Vec<String> = results.iter().map(|r| r.memory.id.clone()).collect();
            if !result_ids.is_empty() {
                tracing::debug!(count = result_ids.len(), "search_access_count_update_start");
                tokio::spawn(async move {
                    if let Err(e) = update_store.batch_bump_access_count(&result_ids).await {
                        tracing::warn!(error = %e, "failed_to_batch_update_access_count_after_search");
                    }
                });
            }
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

    let cross_space_decay_config = state.config.decay_config();

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

        let project_path_filter = sanitized_project_path.clone();
        let accessible_spaces_clone = accessible_space_ids.clone();
        let reranker_clone = state.reranker.clone();
        let decay_cfg = cross_space_decay_config.clone();
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
                conversation_context: None,
                project_path_filter,
            };
            let mut pipeline = RetrievalPipeline::new(store).with_decay_config(decay_cfg);
            if let Some(reranker) = reranker_clone {
                pipeline = pipeline.with_reranker(reranker);
            }
            let result = pipeline.search(&request, None).await;
            (space_id, weight, result)
        });
    }

    let mut all_results: Vec<(Memory, f32, String, Option<String>, Option<String>)> = Vec::new();
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
                    all_results.push((r.memory, weighted, space_id.clone(), r.refine_relevance, r.refine_reasoning));
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
        .map(|(memory, score, _space_id, refine_relevance, refine_reasoning)| SearchResultDto {
            memory,
            score,
            stale_info: None,
            refine_relevance,
            refine_reasoning,
        })
        .collect();

    if params.check_stale {
        for result in &mut results {
            result.stale_info = check_stale_for_memory(&result.memory, &state.store_manager).await;
        }
    }

    // Fire-and-forget: batch increment access_count for cross-space search results
    {
        let mgr = state.store_manager.clone();
        let memories_by_store: Vec<(String, Vec<String>)> = {
            let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
            for r in &results {
                map.entry(r.memory.space_id.clone())
                    .or_default()
                    .push(r.memory.id.clone());
            }
            map.into_iter().collect()
        };
        tracing::debug!(store_count = memories_by_store.len(), "cross_space_access_count_update_start");
        tokio::spawn(async move {
            for (space_id, ids) in memories_by_store {
                if let Ok(store) = mgr.get_store(&space_id).await {
                    if let Err(e) = store.batch_bump_access_count(&ids).await {
                        tracing::warn!(space_id = %space_id, error = %e, "failed_to_batch_update_access_count_after_cross_space_search");
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
        let tier_config = state.config.tier_config();
        let decay_config = state.config.decay_config();
        let new_tier = TierManager::new(tier_config, DecayEngine::new(decay_config)).evaluate_tier(&memory);
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

    if let Some(cat_str) = body.category {
        memory.category = cat_str
            .parse()
            .map_err(|e: String| OmemError::Validation(e))?;
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

    if let Some(sid) = body.session_id {
        memory.session_id = Some(sid);
    }

    if let Some(pp) = body.project_path {
        memory.project_path = Some(sanitize_project_path(&pp).map_err(|e| {
            OmemError::Validation(format!("invalid project_path: {e}"))
        })?);
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
        project_path: params.project_path,
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

pub async fn list_project_paths(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<Vec<String>>, OmemError> {
    let space_id = personal_space_id(&auth.tenant_id);
    let store = state
        .store_manager
        .get_store(&space_id)
        .await?;
    let paths = store
        .list_project_paths()
        .await
        .map_err(|e| OmemError::Internal(format!("failed to list project paths: {e}")))?;
    Ok(Json(paths))
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

// ── Backfill Project Path ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct BackfillProjectPathBody {
    pub project_path: String,
}

pub async fn backfill_project_path(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<BackfillProjectPathBody>,
) -> Result<Json<serde_json::Value>, OmemError> {
    if body.project_path.is_empty() {
        return Err(OmemError::Validation("project_path cannot be empty".to_string()));
    }

    let sanitized = sanitize_project_path(&body.project_path).map_err(|e| {
        OmemError::Validation(format!("invalid project_path: {e}"))
    })?;

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    // Skip private (global by design) and preferences (cross-project by nature)
    let filter = "project_path IS NULL AND visibility != 'private' AND category != 'preferences'";
    let updated_count = store.batch_update_project_path(&sanitized, filter).await?;

    Ok(Json(serde_json::json!({ "updated_count": updated_count })))
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
        project_path: None,
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

    let memories = store.list_all_active(None).await?;
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
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize)]
struct SessionTopicSummary {
    topic: String,
    summary: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    detail: Option<String>,
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
    let project_name = body.project_name.as_deref().map(|name| {
        name.chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .take(32)
            .collect::<String>()
    }).filter(|s| !s.is_empty());
    tracing::info!(raw_project_name = ?body.project_name, clean_project_name = ?project_name, "session_ingest: project_name received");
    let project_path = match body.project_path.as_deref() {
        Some(pp) if !pp.is_empty() => Some(sanitize_project_path(pp).map_err(|e| {
            OmemError::Validation(format!("invalid project_path: {e}"))
        })?),
        _ => None,
    };

    // Fire-and-forget: process in background, return 202 immediately
    tokio::spawn(async move {
        // Acquire per-session lock inside background task
        let lock_arc = {
            let mut entry = state
                .session_locks
                .entry(session_key.clone())
                .or_insert_with(|| (Arc::new(tokio::sync::Mutex::new(())), std::time::Instant::now()));
            entry.value_mut().1 = std::time::Instant::now();
            entry.value().0.clone()
        };
        let _session_guard = lock_arc.lock().await;
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

        // 获取同session的EMOTIONAL记忆摘要（避免重复提取 + 用于追加）
        let emotional_summary = fetch_session_emotional_memory(
            &store,
            session_id.as_deref(),
            Some(5),
        ).await;

        // 获取同session的WORK记忆摘要
        let work_summary = fetch_session_work_memory(
            &store,
            session_id.as_deref(),
            Some(5),
        ).await;

        // 加载最新一条完整 Memory 用于追加逻辑
        let mut existing_emotional = if let Some(ref s) = emotional_summary {
            if let Some(d) = s.memories.first() {
                store.get_by_id(&d.id).await.ok().flatten()
            } else {
                None
            }
        } else {
            None
        };

        let mut existing_work_memory = if let Some(ref s) = work_summary {
            if let Some(d) = s.memories.first() {
                store.get_by_id(&d.id).await.ok().flatten()
            } else {
                None
            }
        } else {
            None
        };

        // 构建合并摘要传给LLM，避免重复提取
        let mut combined_summary_parts = Vec::new();
        if let Some(ref es) = emotional_summary {
            if !es.merged_summary.is_empty() {
                combined_summary_parts.push(format!("[EMOTIONAL memories]\n{}", es.merged_summary));
            }
        }
        if let Some(ref ws) = work_summary {
            if !ws.merged_summary.is_empty() {
                combined_summary_parts.push(format!("[WORK memories]\n{}", ws.merged_summary));
            }
        }
        let combined_summary = if combined_summary_parts.is_empty() {
            None
        } else {
            Some(combined_summary_parts.join("\n\n"))
        };

        let categories = state.category_registry.get_active_categories(&auth.tenant_id).unwrap_or_default();
        let (system_prompt, user_prompt) = crate::ingest::prompts::build_session_extract_prompt_with_memories(
            &conversation,
            combined_summary.as_deref(),
            project_name.as_deref(),
            &categories,
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
        let mut refined_texts: Vec<String> = Vec::new();
        let mut work_original_parent_id: Option<String> = None;
        let mut emotional_original_parent_id: Option<String> = None;

        for (i, topic) in topics.iter().enumerate() {
            let memory_type_raw = topic.memory_type.as_deref().unwrap_or_else(|| {
                // Fallback: scope-based only, no category→memory_type promotion
                // (was: category=preferences auto-promoted to PREFERENCE, causing WORK misclassification)
                if topic.scope == "private" {
                    "EMOTIONAL"
                } else {
                    "WORK"
                }
            });
            // Validate: LLM may return invalid values (e.g. "pinned"). Force to valid set.
            let memory_type = match memory_type_raw {
                "EMOTIONAL" | "WORK" => memory_type_raw,
                _ => {
                    tracing::warn!(invalid_memory_type = %memory_type_raw, "LLM returned invalid memory_type, falling back to scope-based");
                    if topic.scope == "private" { "EMOTIONAL" } else { "WORK" }
                }
            };

            let category: Category = topic.category.as_deref().and_then(|c| {
                let lower = c.to_lowercase();
                let normalized = state
                    .category_registry
                    .normalize(&auth.tenant_id, &lower)
                    .ok()
                    .flatten()
                    .unwrap_or(lower);
                normalized.parse().ok()
            }).unwrap_or_else(|| {
                if topic.scope == "private" {
                    Category::new("profile")
                } else {
                    Category::new("events")
                }
            });

            let summary = if topic.summary.chars().count() > 800 {
                let truncated: String = topic.summary.chars().take(797).collect();
                format!("{}...", truncated)
            } else {
                topic.summary.clone()
            };
            // Use overview/detail fields if provided by LLM, otherwise fallback to truncated summary
            let l1_overview = match &topic.overview {
                Some(o) if !o.is_empty() => o.clone(),
                _ => {
                    let s = &summary;
                    if s.chars().count() <= 150 { s.clone() } else {
                        let truncated: String = s.chars().take(147).collect();
                        format!("{}...", truncated)
                    }
                }
            };
            let l2_content = match &topic.detail {
                Some(d) if !d.is_empty() => d.clone(),
                _ => {
                    let s = &summary;
                    if s.chars().count() <= 500 { s.clone() } else {
                        let truncated: String = s.chars().take(497).collect();
                        format!("{}...", truncated)
                    }
                }
            };
            let tags = {
                let mut t = topic.tags.clone();
                t.dedup();
                t.truncate(3); // preserve semantic tags, leave room for system tags
                if !t.contains(&"session_ingest".to_string()) {
                    t.push("session_ingest".to_string());
                }
                t
            };

            let mut memory = Memory::new(
                &summary,
                category,
                MemoryType::Pinned,
                &tenant_id,
            );
            memory.l0_abstract = topic.topic.clone();
            memory.l1_overview = l1_overview;
            memory.l2_content = l2_content;
            memory.source = Some("session_ingest".to_string());
            memory.session_id = session_id.clone();
            memory.agent_id = agent_id.clone();
            memory.tags = tags.clone();
            memory.project_path = project_path.clone();
            if topic.scope == "private" {
                memory.scope = "private".to_string();
                memory.visibility = "private".to_string();
                // Private memories are global — never bound to a project_path
                memory.project_path = None;
            }

            let apply_append = |mem: &mut crate::domain::memory::Memory, new_content: &str, tags: &[String], topic_title: &str, topic_overview: Option<&str>, topic_detail: Option<&str>| {
                mem.content = new_content.to_string();
                // Preserve the latest topic title (with project prefix) as l0_abstract
                mem.l0_abstract = topic_title.to_string();
                // Use overview/detail fields if provided, otherwise fallback to truncated content
                mem.l1_overview = match topic_overview {
                    Some(o) if !o.is_empty() => o.to_string(),
                    _ => {
                        if new_content.chars().count() <= 150 {
                            new_content.to_string()
                        } else {
                            format!("{}...", new_content.chars().take(147).collect::<String>())
                        }
                    }
                };
                mem.l2_content = match topic_detail {
                    Some(d) if !d.is_empty() => d.to_string(),
                    _ => {
                        if new_content.chars().count() <= 500 {
                            new_content.to_string()
                        } else {
                            format!("{}...", new_content.chars().take(497).collect::<String>())
                        }
                    }
                };
                for tag in tags {
                    if !mem.tags.contains(tag) {
                        mem.tags.push(tag.clone());
                    }
                }
            };

            /// Find the best split point in content so that the first half ≤ max_chars.
            /// Prefers splitting at `## ` heading boundaries; falls back to char-based split.
            /// Returns (first_half, second_half) where second_half contains at least one full section.
            fn find_split_point(content: &str, max_chars: usize) -> (String, String) {
                let mut heading_positions: Vec<usize> = Vec::new();
                let mut byte_offset = 0;
                for line in content.lines() {
                    if line.starts_with("## ") {
                        heading_positions.push(byte_offset);
                    }
                    byte_offset += line.len();
                    if byte_offset < content.len() {
                        byte_offset += 1;
                    }
                }

                let mut best_split_byte: Option<usize> = None;
                for &pos in &heading_positions {
                    let first_half = &content[..pos];
                    let char_count = first_half.trim_end().chars().count();
                    if char_count <= max_chars {
                        best_split_byte = Some(pos);
                    } else {
                        break;
                    }
                }

                let (first, second) = if let Some(split_byte) = best_split_byte {
                    let first_part = content[..split_byte].trim_end().to_string();
                    let second_part = content[split_byte..].trim_start().to_string();
                    (first_part, second_part)
                } else {
                    let char_idx = content.char_indices().take_while(|(idx, _)| *idx < max_chars).last();
                    let split_byte = char_idx.map(|(idx, c)| idx + c.len_utf8()).unwrap_or(content.len().min(max_chars));
                    let first_part = content[..split_byte].trim_end().to_string();
                    let second_part = content[split_byte..].trim_start().to_string();
                    (first_part, second_part)
                };

                (first, second)
            }

            /// Split an over-sized memory: truncate original, create continuation memory.
            /// Returns the new continuation memory ID on success.
            async fn split_memory(
                original: &mut crate::domain::memory::Memory,
                new_content: &str,
                chain_head_id: &str,
                state: &crate::api::server::AppState,
                store: &crate::store::lancedb::LanceStore,
            ) -> Option<String> {
                let (first_half, second_half) = find_split_point(new_content, 3000);

                if second_half.is_empty() {
                    tracing::warn!(
                        memory_id = %original.id,
                        "session_ingest: split resulted in empty second half, skipping split"
                    );
                    return None;
                }

                original.content = first_half.clone();
                if first_half.chars().count() <= 150 {
                    original.l1_overview = first_half.clone();
                } else {
                    original.l1_overview = format!("{}...", first_half.chars().take(147).collect::<String>());
                }
                if first_half.chars().count() <= 500 {
                    original.l2_content = first_half.clone();
                } else {
                    original.l2_content = format!("{}...", first_half.chars().take(497).collect::<String>());
                }

                let original_vec = match state.embed.embed(&[first_half.clone()]).await {
                    Ok(vecs) => vecs.into_iter().next(),
                    Err(e) => {
                        tracing::warn!(error = %e, "session_ingest: failed to re-embed truncated WORK memory");
                        None
                    }
                };
                if let Err(e) = store.update(original, original_vec.as_deref()).await {
                    tracing::warn!(error = %e, memory_id = %original.id, "session_ingest: failed to update truncated WORK memory");
                    return None;
                }
                tracing::info!(
                    memory_id = %original.id,
                    first_chars = first_half.chars().count(),
                    "session_ingest: truncated WORK memory for split"
                );

                let now = chrono::Utc::now().to_rfc3339();
                let new_id = uuid::Uuid::new_v4().to_string();
                let new_mem = crate::domain::memory::Memory {
                    id: new_id.clone(),
                    content: second_half.clone(),
                    l0_abstract: original.l0_abstract.clone(),
                    l1_overview: if second_half.chars().count() <= 150 {
                        second_half.clone()
                    } else {
                        format!("{}...", second_half.chars().take(147).collect::<String>())
                    },
                    l2_content: if second_half.chars().count() <= 500 {
                        second_half.clone()
                    } else {
                        format!("{}...", second_half.chars().take(497).collect::<String>())
                    },
                    category: original.category.clone(),
                    memory_type: original.memory_type.clone(),
                    state: original.state.clone(),
                    tier: original.tier.clone(),
                    importance: original.importance,
                    confidence: original.confidence,
                    access_count: 0,
                    tags: original.tags.clone(),
                    scope: original.scope.clone(),
                    agent_id: original.agent_id.clone(),
                    session_id: original.session_id.clone(),
                    tenant_id: original.tenant_id.clone(),
                    source: Some("auto-split".to_string()),
                    relations: vec![crate::domain::relation::MemoryRelation {
                        relation_type: crate::domain::relation::RelationType::Continues,
                        target_id: chain_head_id.to_string(),
                        context_label: Some("auto-split on overflow".to_string()),
                    }],
                    superseded_by: None,
                    invalidated_at: None,
                    created_at: now.clone(),
                    updated_at: now,
                    last_accessed_at: None,
                    space_id: original.space_id.clone(),
                    visibility: original.visibility.clone(),
                    owner_agent_id: original.owner_agent_id.clone(),
                    provenance: original.provenance.clone(),
                    version: original.version,
                    tier_history: original.tier_history.clone(),
                    cluster_id: original.cluster_id.clone(),
                    is_cluster_anchor: false,
                    metadata: original.metadata.clone(),
                    project_path: original.project_path.clone(),
                };

                let new_vec = match state.embed.embed(&[second_half]).await {
                    Ok(vecs) => vecs.into_iter().next(),
                    Err(e) => {
                        tracing::warn!(error = %e, "session_ingest: failed to embed split continuation WORK memory");
                        None
                    }
                };

                if let Err(e) = store.create(&new_mem, new_vec.as_deref()).await {
                    tracing::warn!(error = %e, "session_ingest: failed to create split continuation WORK memory");
                    return None;
                }
                tracing::info!(
                    new_memory_id = %new_id,
                    second_chars = new_mem.content.chars().count(),
                    "session_ingest: created WORK continuation memory from split"
                );

                Some(new_id)
            }

            if memory_type == "EMOTIONAL" {
                let today = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap()).format("%Y-%m-%d %H:%M").to_string();
                let append_section = format!("\n\n## {} {}\n{}", today, topic.topic, summary);

                let mut appended = false;

                if let Some(mut existing) = existing_emotional.clone() {
                    // Topic-aware matching: append if same topic OR same session
                    // (LLM may give different topic names to the same session's content)
                    let topic_matches = existing.content.lines().any(|line| {
                        line.starts_with("## ") && line.ends_with(&topic.topic)
                    }) || existing.l0_abstract == topic.topic
                      || existing.session_id.as_deref() == session_id.as_deref();
                    if !topic_matches {
                        tracing::info!(
                            existing_id = %existing.id,
                            existing_topic = %existing.l0_abstract,
                            new_topic = %topic.topic,
                            "session_ingest: EMOTIONAL topic mismatch, creating new memory"
                        );
                    } else if existing.content.contains(&summary) {
                            tracing::info!(
                                memory_id = %existing.id,
                                "session_ingest: skipping EMOTIONAL append, content already exists"
                            );
                        } else {
                        let new_content = format!("{}{}", existing.content, append_section);
                        if new_content.chars().count() <= 3000 {
                            apply_append(&mut existing, &new_content, &topic.tags, &topic.topic, topic.overview.as_deref(), topic.detail.as_deref());
                            if let Err(e) = store.update(&existing, None).await {
                                tracing::warn!(error = %e, "session_ingest: failed to append to existing emotional memory");
                            } else {
                                tracing::info!(memory_id = %existing.id, "session_ingest: appended to existing emotional memory (same topic)");
                                refined_texts.push(new_content.clone());
                                existing_emotional = Some(existing);
                                appended = true;
                            }
                        }
                    }
                }

                if !appended {
                    if let Some(ref es) = emotional_summary {
                        let skip_id = existing_emotional.as_ref().map(|e| e.id.clone());
                        let digests: Vec<_> = es.memories.iter()
                            .filter(|d| skip_id.as_ref().map_or(true, |sid| d.id != *sid))
                            .collect();
                        let mut loaded = Vec::new();
                        for d in digests {
                            if let Some(mem) = store.get_by_id(&d.id).await.ok().flatten() {
                                loaded.push(mem);
                            }
                        }
                        loaded.retain(|m| {
                            m.content.lines().any(|line| line.starts_with("## ") && line.ends_with(&topic.topic))
                                || m.l0_abstract == topic.topic
                                || m.session_id.as_deref() == session_id.as_deref()
                        });
                        loaded.sort_by_key(|m| m.content.chars().count());

                        for mut mem in loaded {
                            let new_content = format!("{}{}", mem.content, append_section);
                            if new_content.chars().count() <= 3000 {
                                apply_append(&mut mem, &new_content, &topic.tags, &topic.topic, topic.overview.as_deref(), topic.detail.as_deref());
                                if let Err(e) = store.update(&mem, None).await {
                                    tracing::warn!(error = %e, "session_ingest: failed to append to fallback emotional memory");
                                    continue;
                                }
                                tracing::info!(memory_id = %mem.id, "session_ingest: appended to fallback emotional memory (same topic, shortest fit)");
                                refined_texts.push(new_content.clone());
                                existing_emotional = Some(mem);
                                appended = true;
                                break;
                            }
                        }
                    }
                    if !appended {
                        if emotional_original_parent_id.is_none() {
                            emotional_original_parent_id = existing_emotional.as_ref().map(|m| m.id.clone());
                        }
                        tracing::info!(topic = %topic.topic, "session_ingest: no matching emotional memory for topic, creating new");
                    }
                }

                if appended {
                    continue;
                }
            }

            if memory_type == "WORK" {
                // ── 精炼路径：非private → 尝试LLM精炼 ──
                if topic.scope != "private" {
                    // B1: If no existing_work_memory tracked, search for similar WORK memory in same session
                    if existing_work_memory.is_none() {
                        let sid = session_id.as_deref().unwrap_or("default");
                        let tid = &tenant_id;
                        match crate::ingest::refine_service::find_similar_work_memory(
                            &store, &state.embed, &topic.topic, sid, tid,
                        ).await {
                            Ok(Some(similar)) => {
                                tracing::info!(
                                    similar_id = %similar.id,
                                    score_topic = %topic.topic,
                                    "session_ingest: found similar WORK memory via embedding search"
                                );
                                existing_work_memory = Some(similar);
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!(error = %e, "session_ingest: embedding search failed, skipping refine");
                            }
                        }
                    }

                    if let Some(ref existing) = existing_work_memory {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            async {
                                let chain = collect_chain_memories(&store, existing).await?;
                                refine_and_replace(&store, &state.llm, &state.embed, existing, &chain, &summary, &topic.topic).await
                            }
                        ).await {
                            Ok(Ok(refined)) => {
                                tracing::info!(
                                    memory_id = %refined.id,
                                    new_len = refined.content.chars().count(),
                                    "session_ingest: WORK refined successfully"
                                );
                                if refined.content.chars().count() > 3000 {
                                    tracing::warn!(
                                        len = refined.content.chars().count(),
                                        "session_ingest: refined content exceeds 3000 chars, should have been truncated by refine_service"
                                    );
                                }
                                refined_texts.push(refined.content.clone());
                                existing_work_memory = Some(refined);
                                continue;
                            }
                            Ok(Err(e)) => {
                                tracing::warn!(error = %e, "session_ingest: refine failed, falling back to append");
                            }
                            Err(_) => {
                                tracing::warn!("session_ingest: refine timed out (30s), falling back to append");
                            }
                        }
                    }
                }

                // ── 原有追加逻辑（fallback + 首次创建路径）──
                let today = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap()).format("%Y-%m-%d %H:%M").to_string();
                let section_header = format!("## {} {}", today, topic.topic);
                let section_body = summary.clone();

                let mut appended = false;

                if let Some(mut existing) = existing_work_memory.clone() {
                    let topic_marker = &topic.topic;
                    let has_matching_section = existing.content.lines()
                        .any(|line| line.starts_with("## ") && line.ends_with(topic_marker))
                        || existing.session_id.as_deref() == session_id.as_deref();

                    if !has_matching_section {
                        tracing::info!(
                            existing_id = %existing.id,
                            new_topic = %topic.topic,
                            "session_ingest: WORK topic mismatch, creating new memory"
                        );
                    } else {
                        let new_content = {
                            let mut replaced = false;
                            let mut result = String::new();
                            let mut in_matching_section = false;
                            for line in existing.content.lines() {
                                if line.starts_with("## ") && line.ends_with(topic_marker) {
                                    in_matching_section = true;
                                    result.push_str(&format!("\n\n{}", section_header));
                                    result.push('\n');
                                    result.push_str(&section_body);
                                    replaced = true;
                                    continue;
                                }
                                if in_matching_section && line.starts_with("## ") {
                                    in_matching_section = false;
                                }
                                if !in_matching_section {
                                    if !result.is_empty() && !result.ends_with('\n') {
                                        result.push('\n');
                                    }
                                    result.push_str(line);
                                }
                            }
                            if replaced { result } else if existing.content.contains(&section_body) {
                                tracing::info!(
                                    memory_id = %existing.id,
                                    "session_ingest: skipping WORK append, content already exists"
                                );
                                existing.content.clone()
                            } else {
                                format!("{}\n\n{}\n{}", existing.content, section_header, section_body)
                            }
                        };
                        if new_content.chars().count() <= 3000 {
                            apply_append(&mut existing, &new_content, &topic.tags, &topic.topic, topic.overview.as_deref(), topic.detail.as_deref());
                            if let Err(e) = store.update(&existing, None).await {
                                tracing::warn!(error = %e, "session_ingest: failed to append to existing WORK memory");
                            } else {
                                tracing::info!(memory_id = %existing.id, "session_ingest: updated WORK memory (same topic)");
                                refined_texts.push(new_content.clone());
                                existing_work_memory = Some(existing);
                                appended = true;
                            }
                        } else {
                            let head_id = work_original_parent_id.as_deref().unwrap_or(&existing.id).to_string();
                            if let Some(child_id) = split_memory(&mut existing, &new_content, &head_id, &state, &store).await {
                                if work_original_parent_id.is_none() {
                                    work_original_parent_id = Some(existing.id.clone());
                                }
                                add_continued_by_relation(&store, &head_id, &child_id, "WORK").await;
                                refined_texts.push(new_content.clone());
                                existing_work_memory = Some(existing);
                                appended = true;
                            }
                        }
                    }
                }

                if !appended {
                    if let Some(ref ws) = work_summary {
                        let skip_id = existing_work_memory.as_ref().map(|e| e.id.clone());
                        let digests: Vec<_> = ws.memories.iter()
                            .filter(|d| skip_id.as_ref().map_or(true, |sid| d.id != *sid))
                            .collect();
                        let mut loaded = Vec::new();
                        for d in digests {
                            if let Some(mem) = store.get_by_id(&d.id).await.ok().flatten() {
                                loaded.push(mem);
                            }
                        }
                        let topic_marker = &topic.topic;
                        loaded.retain(|m| {
                            m.content.lines().any(|line| line.starts_with("## ") && line.ends_with(topic_marker.as_str()))
                                || m.l0_abstract == *topic_marker
                                || m.session_id.as_deref() == session_id.as_deref()
                        });
                        loaded.sort_by_key(|m| m.content.chars().count());

                        for mut mem in loaded {
                            let new_content = format!("{}\n\n{}\n{}", mem.content, section_header, section_body);
                            if new_content.chars().count() <= 3000 {
                                apply_append(&mut mem, &new_content, &topic.tags, &topic.topic, topic.overview.as_deref(), topic.detail.as_deref());
                                if let Err(e) = store.update(&mem, None).await {
                                    tracing::warn!(error = %e, "session_ingest: failed to append to fallback WORK memory");
                                    continue;
                                }
                                tracing::info!(memory_id = %mem.id, "session_ingest: appended to fallback WORK memory (same topic, shortest fit)");
                                refined_texts.push(new_content.clone());
                                existing_work_memory = Some(mem);
                                appended = true;
                                break;
                            } else {
                                let head_id = work_original_parent_id.as_deref().unwrap_or(&mem.id).to_string();
                                if let Some(child_id) = split_memory(&mut mem, &new_content, &head_id, &state, &store).await {
                                    if work_original_parent_id.is_none() {
                                        work_original_parent_id = Some(mem.id.clone());
                                    }
                                    add_continued_by_relation(&store, &head_id, &child_id, "WORK").await;
                                    refined_texts.push(new_content.clone());
                                    existing_work_memory = Some(mem);
                                    appended = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !appended {
                        if work_original_parent_id.is_none() {
                            work_original_parent_id = existing_work_memory.as_ref().map(|m| m.id.clone());
                        }
                        tracing::info!(topic = %topic.topic, "session_ingest: no matching WORK memory for topic, creating new");
                    }
                }

                if appended {
                    continue;
                }
            }

            // ── WORK overflow: add Continues relation before create ──
            if memory_type == "WORK" {
                if let Some(ref parent_id) = work_original_parent_id {
                    memory.relations.push(crate::domain::relation::MemoryRelation {
                        relation_type: crate::domain::relation::RelationType::Continues,
                        target_id: parent_id.clone(),
                        context_label: Some("auto-split on overflow".to_string()),
                    });
                    tracing::info!(parent = %parent_id, "WORK: will create with Continues relation to original parent");
                }
            }

            if memory_type == "EMOTIONAL" {
                if let Some(ref parent_id) = emotional_original_parent_id {
                    memory.relations.push(crate::domain::relation::MemoryRelation {
                        relation_type: crate::domain::relation::RelationType::Continues,
                        target_id: parent_id.clone(),
                        context_label: Some("auto-split on overflow".to_string()),
                    });
                    tracing::info!(parent = %parent_id, "EMOTIONAL: will create with Continues relation to original parent");
                }
            }

            let vector = vectors.get(i).cloned();
            if let Err(e) = store.create(&memory, vector.as_deref()).await {
                tracing::error!(error = %e, "session_ingest_bg: create failed");
                return;
            }
            stored += 1;

            // ── Update existing_* so subsequent topics in same batch can append ──
            if memory_type == "WORK" {
                existing_work_memory = Some(memory.clone());
            }
            if memory_type == "EMOTIONAL" {
                existing_emotional = Some(memory.clone());
            }

            // ── Add continued_by reverse relation on parent (bidirectional link) ──
            async fn add_continued_by_relation(
                store: &crate::store::lancedb::LanceStore,
                parent_id: &str,
                child_id: &str,
                label: &str,
            ) {
                const MAX_RETRIES: usize = 3;
                for attempt in 0..MAX_RETRIES {
                    let parent = match store.get_by_id(parent_id).await {
                        Ok(Some(p)) => p,
                        Ok(None) => break, // parent deleted — skip
                        Err(e) => {
                            tracing::warn!(error = %e, parent = %parent_id, attempt, "{label}: failed to get parent for continued_by");
                            break;
                        }
                    };
                    let mut updated = parent.clone();
                    // Idempotent check: skip if relation already exists
                    if updated.relations.iter().any(|r|
                        r.relation_type == crate::domain::relation::RelationType::ContinuedBy && r.target_id == child_id
                    ) {
                        tracing::debug!(parent = %parent_id, child = %child_id, "{label}: continued_by relation already exists, skipping");
                        break;
                    }
                    updated.relations.push(crate::domain::relation::MemoryRelation {
                        relation_type: crate::domain::relation::RelationType::ContinuedBy,
                        target_id: child_id.to_string(),
                        context_label: Some("auto-split continuation".to_string()),
                    });
                    match store.update(&updated, None).await {
                        Ok(_) => {
                            tracing::info!(parent = %parent_id, child = %child_id, "{label}: added continued_by relation to parent");
                            break;
                        }
                        Err(e) if attempt < MAX_RETRIES - 1 => {
                            tracing::warn!(error = %e, parent = %parent_id, attempt, "{label}: retry continued_by update");
                            continue;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, parent = %parent_id, "{label}: failed to add continued_by relation after retries");
                            break;
                        }
                    }
                }
            }

            if memory_type == "WORK" {
                if let Some(ref parent_id) = work_original_parent_id {
                    add_continued_by_relation(&store, parent_id, &memory.id, "WORK").await;
                }
            }
            if memory_type == "EMOTIONAL" {
                if let Some(ref parent_id) = emotional_original_parent_id {
                    add_continued_by_relation(&store, parent_id, &memory.id, "EMOTIONAL").await;
                }
            }

            created_memories.push((memory.clone(), vector.clone()));
        }

        // --- Profile V2 Induction Trigger ---
        let engine = state.induction_engine.clone();
        let mut ind_texts: Vec<String> = created_memories.iter().map(|(m, _)| m.content.clone()).collect();
        ind_texts.extend(refined_texts);
        let ind_tenant = tenant_id.clone();
        if !ind_texts.is_empty() {
            tracing::debug!(texts_count = ind_texts.len(), "triggering profile induction from session_ingest");
            tokio::spawn(async move {
                match engine.trigger_induction(&ind_tenant, "session_ingest", &ind_texts).await {
                    Ok(Some(result)) => tracing::info!(run_id = %result.run_id, extracted = result.extracted_count, "session_ingest: profile_induction_triggered"),
                    Ok(None) => tracing::debug!("session_ingest: profile_induction_skipped"),
                    Err(e) => tracing::warn!(error = %e, "session_ingest: profile_induction_failed"),
                }
            });
        }

        tracing::info!(
            stored = stored,
            tenant = %tenant_id,
            "session_ingest: created independent memories"
        );

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

pub fn clean_message_content(content: &str) -> String {
    let mut cleaned = content.to_string();

    // Remove XML-like system tags and their content
    let xml_patterns = [
        "<system-reminder>",
        "<auto-slash-command>",
        "<thinking>",
        "< omoc:",
        "<analysis>",
        "<ultrawork-mode>",
        "<cerebro-context>",
        "<cerebro-profile>",
        "<cerebro-fetch-policy>",
        "<EXTREMELY_IMPORTANT>",
        "<SUBAGENT-STOP>",
        "<team_mode_status>",
        "<dcp_message>",
    ];
    for pattern in xml_patterns {
        while let Some(start) = cleaned.find(pattern) {
            let tag_name_end = cleaned[start..]
                .find(|c: char| [' ', '>'].contains(&c))
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
        "[restore checkpointed session agent configuration after compaction]",
        "[session recovered - continuing previous task]",
        "<!-- OMO_INTERNAL_INITIATOR -->",
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



/// Fetch up to N recent private (EMOTIONAL) session memories for a session.
/// Returns a merged summary instead of a single memory to provide richer context
/// and avoid duplicate extractions.
/// No tags filter — matches by session_id + scope to support both session_ingest and session_compress sources.
async fn fetch_session_emotional_memory(
    store: &crate::store::lancedb::LanceStore,
    session_id: Option<&str>,
    limit: Option<usize>,
) -> Option<crate::domain::memory::SessionMemorySummary> {
    let sid = session_id?;
    let limit = limit.unwrap_or(5);

    let filter = ListFilter {
        ..Default::default()
    };

    let memories = match store.list_filtered(&filter, 100, 0).await {
        Ok(mems) => mems
            .into_iter()
            .filter(|m| m.session_id.as_deref() == Some(sid) && m.scope == "private")
            .collect::<Vec<_>>(),
        Err(_) => return None,
    };

    if memories.is_empty() {
        return None;
    }

    let total_count = memories.len();
    let mut sorted = memories;
    sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let recent: Vec<_> = sorted.into_iter().take(limit).collect();

    let digests: Vec<crate::domain::memory::MemoryDigest> = recent
        .iter()
        .map(|m| crate::domain::memory::MemoryDigest {
            id: m.id.clone(),
            title: m.l0_abstract.clone(),
            category: m.category.clone(),
            tags: m.tags.clone(),
            content_preview: m.content.chars().take(200).collect(),
            updated_at: m.updated_at.clone(),
        })
        .collect();

    let merged_summary = build_merged_summary(&recent);

    Some(crate::domain::memory::SessionMemorySummary {
        memories: digests,
        merged_summary,
        total_count,
    })
}

/// Fetch up to N recent public (WORK) session memories for a session.
/// No tags filter — matches by session_id + scope to support both session_ingest and session_compress sources.
async fn fetch_session_work_memory(
    store: &crate::store::lancedb::LanceStore,
    session_id: Option<&str>,
    limit: Option<usize>,
) -> Option<crate::domain::memory::SessionMemorySummary> {
    let sid = session_id?;
    let limit = limit.unwrap_or(5);

    let filter = ListFilter {
        ..Default::default()
    };

        let memories = match store.list_filtered(&filter, 100, 0).await {
            Ok(mems) => mems
                .into_iter()
                .filter(|m| {
                    m.session_id.as_deref() == Some(sid)
                        && m.scope != "private"
                        && m.category != Category::new("preferences")
                })
                .collect::<Vec<_>>(),
        Err(_) => return None,
    };

    if memories.is_empty() {
        return None;
    }

    let total_count = memories.len();
    let mut sorted = memories;
    sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let recent: Vec<_> = sorted.into_iter().take(limit).collect();

    let digests: Vec<crate::domain::memory::MemoryDigest> = recent
        .iter()
        .map(|m| crate::domain::memory::MemoryDigest {
            id: m.id.clone(),
            title: m.l0_abstract.clone(),
            category: m.category.clone(),
            tags: m.tags.clone(),
            content_preview: m.content.chars().take(200).collect(),
            updated_at: m.updated_at.clone(),
        })
        .collect();

    let merged_summary = build_merged_summary(&recent);

    Some(crate::domain::memory::SessionMemorySummary {
        memories: digests,
        merged_summary,
        total_count,
    })
}

/// Build a merged summary string from a slice of memories, capped at 2000 chars.
fn build_merged_summary(memories: &[Memory]) -> String {
    let mut parts = Vec::new();
    let mut total_len = 0usize;
    for m in memories {
        let part = format!("[{}] {}: {}", m.updated_at, m.l0_abstract, m.l1_overview);
        if total_len + part.len() + 2 > 2000 {
            break;
        }
        total_len += part.len() + 2;
        parts.push(part);
    }
    parts.join("\n\n")
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
