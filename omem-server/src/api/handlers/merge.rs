use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::{personal_space_id, AppState};
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::tenant::AuthInfo;
use crate::ingest::prompts::build_merge_prompt;
use crate::llm::complete_json;

#[derive(Deserialize)]
pub struct MergeMemoriesRequest {
    pub memory_ids: Vec<String>,
    pub strategy: Option<String>,
    pub merged_content: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Serialize)]
pub struct MergeMemoriesResponse {
    pub merged_memory: Memory,
    pub consumed_ids: Vec<String>,
    pub strategy: String,
}

#[derive(Deserialize)]
struct LlmMergeResult {
    l0_abstract: String,
    l1_overview: String,
    l2_content: String,
    category: String,
    tags: Vec<String>,
}

pub async fn merge_memories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<MergeMemoriesRequest>,
) -> Result<impl IntoResponse, OmemError> {
    if body.memory_ids.len() < 2 || body.memory_ids.len() > 10 {
        return Err(OmemError::Validation(
            "memory_ids must contain 2-10 IDs".to_string(),
        ));
    }

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let memories = store.get_memories_by_ids(&body.memory_ids).await?;

    if memories.len() != body.memory_ids.len() {
        return Err(OmemError::Validation(
            "one or more memory IDs not found".to_string(),
        ));
    }

    for mem in &memories {
        if mem.tenant_id != auth.tenant_id {
            return Err(OmemError::Unauthorized(
                "memory does not belong to tenant".to_string(),
            ));
        }
    }

    let strategy = body.strategy.as_deref().unwrap_or("llm");

    if strategy != "llm" && strategy != "manual" {
        return Err(OmemError::Validation(
            "strategy must be 'llm' or 'manual'".to_string(),
        ));
    }

    if strategy == "manual" && body.merged_content.is_none() {
        return Err(OmemError::Validation(
            "merged_content is required for manual strategy".to_string(),
        ));
    }

    let primary_idx = memories
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.importance
                        .partial_cmp(&b.importance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        })
        .map(|(i, _)| i)
        .unwrap_or(0);

    let mut primary = memories[primary_idx].clone();

    let mut all_tags: HashSet<String> = HashSet::new();
    for mem in &memories {
        for tag in &mem.tags {
            all_tags.insert(tag.clone());
        }
    }

    if strategy == "manual" {
        let merged_content = body.merged_content.unwrap();
        primary.content = merged_content.clone();
        primary.l2_content = merged_content.clone();
        primary.l0_abstract = merged_content.chars().take(100).collect();
        let overview_truncated: String = merged_content.chars().take(200).collect();
        primary.l1_overview = if merged_content.chars().count() > 200 {
            format!("{}...", overview_truncated)
        } else {
            overview_truncated
        };
    } else {
        let (system, user) = build_merge_prompt(&memories);
        let result: LlmMergeResult = complete_json::<LlmMergeResult>(state.llm.as_ref(), &system, &user)
            .await
            .map_err(|e| OmemError::Llm(format!("merge LLM failed: {e}")))?;

        primary.l0_abstract = result.l0_abstract;
        primary.l1_overview = result.l1_overview;
        primary.l2_content = result.l2_content.clone();
        primary.content = result.l2_content;
        primary.category = result
            .category
            .parse()
            .map_err(|e: String| OmemError::Validation(e))?;
        for tag in result.tags {
            all_tags.insert(tag);
        }
    }

    primary.tags = all_tags.into_iter().collect();
    primary.updated_at = chrono::Utc::now().to_rfc3339();
    if let Some(agent_id) = body.agent_id {
        primary.agent_id = Some(agent_id);
    }

    let vectors = state
        .embed
        .embed(&[primary.content.clone()])
        .await
        .map_err(|e| OmemError::Embedding(format!("failed to embed merged content: {e}")))?;
    let vector = vectors.into_iter().next();

    store.update(&primary, vector.as_deref()).await?;

    let mut consumed_ids = Vec::new();
    for (i, mem) in memories.iter().enumerate() {
        if i != primary_idx {
            store.hard_delete(&mem.id).await?;
            consumed_ids.push(mem.id.clone());
        }
    }

    Ok(Json(MergeMemoriesResponse {
        merged_memory: primary,
        consumed_ids,
        strategy: strategy.to_string(),
    }))
}
