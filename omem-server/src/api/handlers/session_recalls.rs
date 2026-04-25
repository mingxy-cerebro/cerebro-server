use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::{personal_space_id, AppState};
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::tenant::AuthInfo;
use crate::lifecycle::tier::TierManager;
use crate::store::lancedb::SessionRecall;

const SHOULD_RECALL_SYSTEM_PROMPT: &str = r#"你是一个记忆召回助手。用户有一个个人知识库，保存了过往笔记、项目经验、技术方案、偏好设置、私密记录等记忆。你的任务是判断用户当前的问题是否需要从知识库中检索相关记忆来辅助回答。

回答 yes 的情况：
- 涉及用户个人知识、项目细节、过往经验
- 涉及私密内容、个人情感、亲密关系、家庭事务
- 涉及密码、配置、账号等敏感信息
- 任何可能需要参考历史记录的问题

回答 no 的情况：
- 通用常识、简单问候
- 与历史记录完全无关的闲聊

注意：知识库中包括私密记忆，涉及私密内容的问题同样需要召回。只回答 yes 或 no。"#;

#[derive(Deserialize)]
pub struct ShouldRecallRequest {
    pub query_text: String,
    pub last_query_text: Option<String>,
    pub session_id: String,
    pub similarity_threshold: Option<f32>,
    pub max_results: Option<usize>,
    pub project_tags: Option<Vec<String>>,
    pub agent_id: Option<String>,
}

#[derive(Serialize)]
pub struct MemoryWithScore {
    pub memory: Memory,
    pub score: f32,
}

#[derive(Serialize)]
pub struct ShouldRecallResponse {
    pub should_recall: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memories: Option<Vec<MemoryWithScore>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clustered: Option<crate::cluster::aggregator::ClusteredResult>,
}

#[derive(Deserialize)]
pub struct CreateSessionRecallRequest {
    pub session_id: String,
    pub memory_ids: Vec<String>,
    pub recall_type: String,
    #[serde(default)]
    pub query_text: String,
    #[serde(default)]
    pub similarity_score: f32,
    #[serde(default)]
    pub llm_confidence: f32,
}

#[derive(Deserialize)]
pub struct ListSessionRecallsQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub session_id: Option<String>,
    pub expand: Option<String>,
}

fn default_limit() -> usize {
    20
}

#[derive(Serialize)]
pub struct ListSessionRecallsResponse {
    pub recalls: Vec<SessionRecall>,
    pub limit: usize,
    pub offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memories: Option<Vec<Memory>>,
}

pub async fn should_recall(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<ShouldRecallRequest>,
) -> Result<Json<ShouldRecallResponse>, OmemError> {
    if body.query_text.is_empty() {
        return Err(OmemError::Validation(
            "query_text cannot be empty".to_string(),
        ));
    }

    let similarity_score = if let Some(ref last_query) = body.last_query_text {
        if !last_query.is_empty() {
            let texts = vec![body.query_text.clone(), last_query.clone()];
            let embeddings = state
                .embed
                .embed(&texts)
                .await
                .map_err(|e| OmemError::Embedding(format!("failed to embed query: {e}")))?;

            if embeddings.len() == 2 {
                let sim = cosine_similarity(&embeddings[0], &embeddings[1]);
                if sim > 0.7 {
                    return Ok(Json(ShouldRecallResponse {
                        should_recall: false,
                        reason: Some("similarity_too_high".to_string()),
                        memories: None,
                        confidence: None,
                        similarity_score: Some(sim),
                        clustered: None,
                    }));
                }
                Some(sim)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let denoised_query = denoise_for_recall(&body.query_text);

    let system = SHOULD_RECALL_SYSTEM_PROMPT;
    let user = format!(
        "用户问题：{}\n\n这个问题是否需要从用户个人知识库中检索相关记忆来回答？回答 yes 或 no。",
        denoised_query
    );

    let (llm_yes, _llm_reason) = match state.recall_llm.complete_text(system, &user).await {
        Ok(llm_response) => {
            let trimmed = llm_response.trim().to_lowercase();
            let yes = trimmed.starts_with("yes");
            tracing::info!(query = %body.query_text, llm_response = %trimmed, llm_yes = yes, "recall_llm_response");
            (yes, if yes { "llm_yes" } else { "llm_no" })
        }
        Err(e) => {
            tracing::warn!(query = %body.query_text, error = %e, "recall_llm_error_fallback");
            (true, "llm_error_fallback")
        }
    };

    let vectors = state
        .embed
        .embed(std::slice::from_ref(&denoised_query))
        .await
        .map_err(|e| OmemError::Embedding(format!("failed to embed query: {e}")))?;
    let query_vector = vectors.into_iter().next();

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let mut min_score = body.similarity_threshold.unwrap_or(0.4);
    if min_score < 0.0 || min_score > 1.0 {
        min_score = 0.4;
    }

    let mut max_results = body.max_results.unwrap_or(5);
    if max_results == 0 {
        max_results = 5;
    }

    let is_zero_vector = query_vector.as_ref().map_or(true, |v| v.iter().all(|&x| x == 0.0));

    let effective_min_score = if llm_yes { min_score } else { min_score * 0.5 };

    let spaces = state
        .space_store
        .list_spaces_for_user(&auth.tenant_id)
        .await?;
    let accessible_space_ids: Vec<String> = spaces.iter().map(|s| s.id.clone()).collect();

    let visibility_filter = body
        .agent_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|agent_id| store.build_visibility_filter(agent_id, &accessible_space_ids));
    let vis_ref = visibility_filter.as_deref();

    // Two-phase search: project-first, then global fallback
    let project_tags_slice = body.project_tags.as_deref();
    let mut all_results: Vec<(Memory, f32)> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // Phase 1: Search within project scope (using project_tags filter)
    if let Some(tags) = project_tags_slice {
        if !tags.is_empty() {
            let project_results = if is_zero_vector {
                store
                    .fts_search(&body.query_text, max_results, None, vis_ref, Some(tags))
                    .await
                    .unwrap_or_default()
            } else {
                let search_vec = query_vector.as_ref().unwrap();
                store
                    .vector_search(search_vec, max_results, effective_min_score, None, vis_ref, Some(tags))
                    .await
                    .unwrap_or_default()
            };

            for (mem, score) in project_results {
                seen_ids.insert(mem.id.clone());
                all_results.push((mem, score));
            }

            tracing::info!(
                query = %body.query_text,
                project_results = all_results.len(),
                project_tags = ?tags,
                "should_recall_phase1_project"
            );
        }
    }

    // Phase 2: If project results are insufficient, supplement with global scope
    let need_global = all_results.len() < max_results;
    if need_global {
        let remaining = max_results - all_results.len();
        let global_results = if is_zero_vector {
            store
                .fts_search(&body.query_text, remaining + 5, Some("global"), vis_ref, None)
                .await
                .unwrap_or_default()
        } else {
            let search_vec = query_vector.as_ref().unwrap();
            store
                .vector_search(search_vec, remaining + 5, effective_min_score, Some("global"), vis_ref, None)
                .await
                .unwrap_or_default()
        };

        let mut global_count = 0;
        for (mem, score) in global_results {
            if !seen_ids.contains(&mem.id) {
                seen_ids.insert(mem.id.clone());
                all_results.push((mem, score));
                global_count += 1;
                if global_count >= remaining {
                    break;
                }
            }
        }

        tracing::info!(
            query = %body.query_text,
            global_supplement = global_count,
            total_results = all_results.len(),
            "should_recall_phase2_global"
        );
    }

    // Fallback: if no project_tags provided, do a normal global search
    if project_tags_slice.is_none() || project_tags_slice.map_or(true, |t| t.is_empty()) {
        all_results = if is_zero_vector {
            store
                .fts_search(&body.query_text, max_results, None, vis_ref, None)
                .await
                .unwrap_or_default()
        } else {
            let search_vec = query_vector.as_ref().unwrap();
            store
                .vector_search(search_vec, max_results, effective_min_score, None, vis_ref, None)
                .await
                .unwrap_or_default()
        };
        tracing::info!(query = %body.query_text, "should_recall_no_project_tags_fallback");
    }

    let results = all_results;

    let memories: Vec<MemoryWithScore> = results
        .into_iter()
        .map(|(memory, score)| MemoryWithScore { memory, score })
        .collect();

    tracing::info!(query = %body.query_text, result_count = memories.len(), should_recall = !memories.is_empty(), "should_recall_result");

    let clustered = if !memories.is_empty() {
        let aggregator = crate::cluster::aggregator::ClusterAggregator::new(state.cluster_store.clone());
        match aggregator.aggregate(memories.iter().map(|m| m.memory.clone()).collect()).await {
            Ok(clustered) => {
                tracing::info!(
                    cluster_count = clustered.cluster_summaries.len(),
                    standalone_count = clustered.standalone_memories.len(),
                    "session_recall_aggregated"
                );
                Some(clustered)
            }
            Err(e) => {
                tracing::warn!(error = %e, "session_recall_aggregation_failed");
                None
            }
        }
    } else {
        None
    };

    if memories.is_empty() {
        return Ok(Json(ShouldRecallResponse {
            should_recall: false,
            reason: Some("no_relevant_memories".to_string()),
            memories: None,
            confidence: None,
            similarity_score,
            clustered: None,
        }));
    }

    let confidence = memories.iter().map(|m| m.score).sum::<f32>() / memories.len() as f32;

    // Fire-and-forget: increment access_count and evaluate tier for recalled memories
    {
        let update_store = store;
        let memories_to_update: Vec<Memory> = memories.iter().map(|m| m.memory.clone()).collect();
        tracing::debug!(count = memories_to_update.len(), query = %body.query_text, "recall_access_count_update_start");
        tokio::spawn(async move {
            for mut memory in memories_to_update {
                let old_tier = memory.tier.clone();
                let old_count = memory.access_count;
                memory.access_count += 1;
                memory.last_accessed_at = Some(chrono::Utc::now().to_rfc3339());
                let new_tier = TierManager::with_defaults().evaluate_tier(&memory);
                if new_tier != old_tier {
                    tracing::info!(memory_id = %memory.id, old_tier = %old_tier, new_tier = %new_tier, access_count = old_count + 1, "tier_promoted_via_recall");
                    memory.append_tier_change(&old_tier.to_string(), &new_tier.to_string(), "access_via_recall");
                }
                memory.tier = new_tier;
                if let Err(e) = update_store.update(&memory, None).await {
                    tracing::warn!(memory_id = %memory.id, error = %e, "failed_to_update_access_count_after_recall");
                }
            }
        });
    }

    Ok(Json(ShouldRecallResponse {
        should_recall: true,
        reason: None,
        memories: Some(memories),
        confidence: Some(confidence),
        similarity_score,
        clustered: clustered,
    }))
}

pub async fn create_session_recall(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<CreateSessionRecallRequest>,
) -> Result<Json<Vec<SessionRecall>>, OmemError> {
    if body.session_id.is_empty() {
        return Err(OmemError::Validation(
            "session_id cannot be empty".to_string(),
        ));
    }
    if body.memory_ids.is_empty() {
        return Err(OmemError::Validation(
            "memory_ids cannot be empty".to_string(),
        ));
    }
    if body.recall_type != "auto" && body.recall_type != "manual" {
        return Err(OmemError::Validation(
            "recall_type must be 'auto' or 'manual'".to_string(),
        ));
    }

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let mut recalls = Vec::new();
    for memory_id in body.memory_ids {
        let recall = SessionRecall {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: body.session_id.clone(),
            memory_id,
            recall_type: body.recall_type.clone(),
            query_text: body.query_text.clone(),
            similarity_score: body.similarity_score,
            llm_confidence: body.llm_confidence,
            tenant_id: auth.tenant_id.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        store.create_session_recall(&recall).await?;
        recalls.push(recall);
    }

    Ok(Json(recalls))
}

pub async fn list_session_recalls(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<ListSessionRecallsQuery>,
) -> Result<Json<ListSessionRecallsResponse>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let recalls = store
        .list_session_recalls(
            &auth.tenant_id,
            params.session_id.as_deref(),
            params.limit,
            params.offset,
        )
        .await?;

    let memories = if params.expand.as_deref() == Some("memories") {
        let memory_ids: Vec<String> = recalls.iter().map(|r| r.memory_id.clone()).collect();
        if memory_ids.is_empty() {
            Some(vec![])
        } else {
            Some(store.get_memories_by_ids(&memory_ids).await?)
        }
    } else {
        None
    };

    Ok(Json(ListSessionRecallsResponse {
        recalls,
        limit: params.limit,
        offset: params.offset,
        memories,
    }))
}

pub async fn get_session_recall(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<Json<SessionRecall>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let recall = store
        .get_session_recall_by_id(&id)
        .await?
        .ok_or_else(|| OmemError::NotFound(format!("session_recall {id}")))?;

    if recall.tenant_id != auth.tenant_id {
        return Err(OmemError::Unauthorized("access denied".to_string()));
    }

    Ok(Json(recall))
}

pub async fn delete_session_recall(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let recall = store
        .get_session_recall_by_id(&id)
        .await?
        .ok_or_else(|| OmemError::NotFound(format!("session_recall {id}")))?;

    if recall.tenant_id != auth.tenant_id {
        return Err(OmemError::Unauthorized("access denied".to_string()));
    }

    store.delete_session_recall(&id).await?;

    Ok(Json(serde_json::json!({"deleted": true, "id": id})))
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

fn denoise_for_recall(text: &str) -> String {
    let max_chars = 200;
    let mut cleaned = text.to_string();

    let re_tags = regex::Regex::new(r"<[^>]+>").unwrap_or_else(|_| regex::Regex::new("").unwrap());
    cleaned = re_tags.replace_all(&cleaned, "").to_string();

    let re_code = regex::Regex::new(r"```[\s\S]*?```").unwrap_or_else(|_| regex::Regex::new("").unwrap());
    cleaned = re_code.replace_all(&cleaned, "").to_string();

    let re_ws = regex::Regex::new(r"\s+").unwrap_or_else(|_| regex::Regex::new("").unwrap());
    cleaned = re_ws.replace_all(&cleaned.trim(), " ").to_string();

    if cleaned.chars().count() > max_chars {
        let truncated: String = cleaned.chars().take(max_chars).collect();
        format!("{}...(truncated)", truncated)
    } else {
        cleaned
    }
}
