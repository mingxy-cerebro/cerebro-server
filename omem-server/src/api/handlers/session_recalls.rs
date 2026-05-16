use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::api::server::{personal_space_id, AppState};
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::tenant::AuthInfo;
use crate::store::lancedb::{RecallEvent, RecallItem};
use crate::retrieve::pipeline::{RetrievalPipeline, SearchOverrides, SearchRequest, SearchResult};

type RecallTimeMap = HashMap<String, chrono::DateTime<chrono::Utc>>;
static LAST_RECALL_TIME: LazyLock<Arc<Mutex<RecallTimeMap>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

const SHOULD_RECALL_SYSTEM_PROMPT: &str = r#"你是一个记忆召回助手。用户有一个个人知识库，保存了过往笔记、项目经验、技术方案、偏好设置等记忆。判断用户当前问题是否需要检索知识库。

分类标准：
A类（需要召回 → yes）：明确引用过去的决策、偏好、项目细节、密钥/配置、用"之前/上次/那个"等词引用历史
B类（可能需要 → no）：涉及技术方案选择、架构讨论，但问题是首次提出的新话题
C类（不需要 → no）：通用编程问题、简单功能实现、数学推导、与历史无关的闲聊

规则：
- A类回答 yes
- B类和C类回答 no
- 不确定时答 no（后续有兜底搜索机制，不会遗漏高相关记忆）
- 只回答 yes 或 no"#;

#[derive(Deserialize)]
pub struct ShouldRecallRequest {
    pub query_text: String,
    pub last_query_text: Option<String>,
    pub session_id: String,
    pub similarity_threshold: Option<f32>,
    pub max_results: Option<usize>,
    pub project_tags: Option<Vec<String>>,
    pub agent_id: Option<String>,
    pub conversation_context: Option<Vec<String>>,
    #[serde(default)]
    pub fetch_multiplier: Option<usize>,
    #[serde(default)]
    pub topk_cap_multiplier: Option<usize>,
    #[serde(default)]
    pub mmr_jaccard_threshold: Option<f32>,
    #[serde(default)]
    pub mmr_penalty_factor: Option<f32>,
    #[serde(default)]
    pub phase2_multiplier: Option<usize>,
    #[serde(default)]
    pub llm_max_eval: Option<usize>,
    #[serde(default)]
    pub refine_strategy: Option<String>,
    #[serde(default)]
    pub refine_medium_chars: Option<usize>,
}

#[derive(Serialize)]
pub struct MemoryWithScore {
    pub memory: Memory,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_relevance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_reasoning: Option<String>,
}

#[derive(Serialize)]
pub struct DiscardedItem {
    pub memory_id: String,
    pub content: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_relevance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_reasoning: Option<String>,
}

#[derive(Serialize)]
pub struct ShouldRecallResponse {
    pub should_recall: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memories: Option<Vec<MemoryWithScore>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discarded: Option<Vec<DiscardedItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clustered: Option<crate::cluster::aggregator::ClusteredResult>,
}

fn default_limit() -> usize {
    20
}

#[derive(Deserialize)]
pub struct ListSessionGroupsQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

#[derive(Serialize)]
pub struct SessionGroup {
    pub session_id: String,
    pub count: usize,
    pub auto_count: usize,
    pub manual_count: usize,
    pub last_injected_at: String,
    pub latest_query: String,
}

#[derive(Serialize)]
pub struct ListSessionGroupsResponse {
    pub groups: Vec<SessionGroup>,
    pub total_count: usize,
    pub limit: usize,
    pub offset: usize,
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

    // Sanitize query_text: strip system injection artifacts (server-side fallback)
    let sanitized_query = crate::api::handlers::memory::clean_message_content(&body.query_text);
    if sanitized_query.is_empty() {
        return Ok(Json(ShouldRecallResponse {
            should_recall: false,
            reason: Some("system_injection_filtered".to_string()),
            memories: None,
            discarded: None,
            confidence: None,
            similarity_score: None,
            clustered: None,
        }));
    }

    // Per-session rate limiting
    {
        let mut last_times = LAST_RECALL_TIME.lock().await;
        let now = chrono::Utc::now();
        last_times.retain(|_, dt| now.signed_duration_since(*dt).num_seconds() < 86400);
        let key = if body.session_id.is_empty() {
            auth.tenant_id.clone()
        } else {
            format!("{}:{}", auth.tenant_id, body.session_id)
        };
        if let Some(last_time) = last_times.get(&key) {
            let elapsed = chrono::Utc::now().signed_duration_since(*last_time);
            let min_interval = state.config.should_recall_min_interval_secs as i64;
            if elapsed.num_seconds() < min_interval {
                return Ok(Json(ShouldRecallResponse {
                    should_recall: false,
                    reason: Some("rate_limited".to_string()),
                    memories: None,
                    discarded: None,
                    confidence: None,
                    similarity_score: None,
                    clustered: None,
                }));
            }
        }
        last_times.insert(key, chrono::Utc::now());
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
                        discarded: None,
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
    if !(0.0..=1.0).contains(&min_score) {
        min_score = 0.4;
    }

    let mut max_results = body.max_results.unwrap_or(5);
    if max_results == 0 {
        max_results = 5;
    }

    let (effective_min_score, quality_gate) = if llm_yes {
        (min_score, 0.40)
    } else if has_recall_signals(&denoised_query) {
        tracing::info!(query = %body.query_text, original_min_score = min_score, strict_min_score = min_score.max(0.55), quality_gate = 0.50, "recall_llm_rejected_with_signals");
        (min_score.max(0.55), 0.50)
    } else {
        tracing::info!(query = %body.query_text, strict_min_score = min_score.max(0.65), quality_gate = 0.60, "recall_llm_rejected_strict");
        (min_score.max(0.65), 0.60)
    };

    let spaces = state
        .space_store
        .list_spaces_for_user(&auth.tenant_id)
        .await?;
    let accessible_space_ids: Vec<String> = spaces.iter().map(|s| s.id.clone()).collect();

    // Construct pipeline on-the-fly
    let mut pipeline = RetrievalPipeline::new(store.clone())
        .with_tag_weight(0.2)
        .with_llm(state.recall_llm.clone());
    if let Some(ref reranker) = state.reranker {
        pipeline = pipeline.with_reranker(reranker.clone());
    }

    // Build SearchOverrides: request params override config defaults
    let phase2_multiplier = body.phase2_multiplier.unwrap_or(state.config.recall_phase2_multiplier);
    let overrides = SearchOverrides {
        fetch_multiplier: body.fetch_multiplier.or(Some(state.config.search_fetch_multiplier)),
        topk_cap_multiplier: body.topk_cap_multiplier.or(Some(state.config.search_topk_cap_multiplier)),
        mmr_jaccard_threshold: body.mmr_jaccard_threshold.or(Some(state.config.search_mmr_jaccard_threshold)),
        mmr_penalty_factor: body.mmr_penalty_factor.or(Some(state.config.search_mmr_penalty_factor)),
        llm_max_eval: body.llm_max_eval.or(Some(state.config.recall_llm_max_eval)),
        refine_strategy: body.refine_strategy.clone().or(Some(state.config.recall_refine_strategy.clone())),
        refine_medium_chars: body.refine_medium_chars.or(Some(state.config.recall_refine_medium_chars)),
        refine_timeout_secs: Some(state.config.recall_llm_refine_timeout_secs),
    };

    tracing::info!(
        query = %body.query_text,
        fetch_multiplier = ?overrides.fetch_multiplier,
        topk_cap_multiplier = ?overrides.topk_cap_multiplier,
        mmr_jaccard_threshold = ?overrides.mmr_jaccard_threshold,
        mmr_penalty_factor = ?overrides.mmr_penalty_factor,
        phase2_multiplier,
        llm_max_eval = ?overrides.llm_max_eval,
        refine_strategy = ?overrides.refine_strategy,
        refine_medium_chars = ?overrides.refine_medium_chars,
        refine_timeout_secs = ?overrides.refine_timeout_secs,
        "should_recall_overrides"
    );

    // Two-phase search: project-first, then global fallback
    let mut all_results: Vec<SearchResult> = Vec::new();
    let mut all_discarded: Vec<SearchResult> = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut seen_ids_for_discarded = HashSet::new();
    let project_tags_slice = body.project_tags.as_deref();

    // Phase 1: Search within project scope (using project_tags filter)
    if let Some(tags) = project_tags_slice {
        if !tags.is_empty() {
            let search_req = SearchRequest {
                query: denoised_query.clone(),
                query_vector: query_vector.clone(),
                tenant_id: auth.tenant_id.clone(),
                scope_filter: None,
                limit: Some(max_results),
                min_score: Some(effective_min_score),
                include_trace: false,
                tags_filter: Some(tags.to_vec()),
                source_filter: None,
                agent_id_filter: body.agent_id.clone(),
                accessible_spaces: accessible_space_ids.clone(),
                conversation_context: body.conversation_context.clone(),
            };
            match pipeline.search(&search_req, Some(&overrides)).await {
                Ok(results) => {
                    for d in results.discarded {
                        if seen_ids_for_discarded.insert(d.memory.id.clone()) {
                            all_discarded.push(d);
                        }
                    }
                    let discarded = all_discarded.len();
                    for r in results.results {
                        if seen_ids.insert(r.memory.id.clone()) {
                            all_results.push(r);
                        }
                    }
                    tracing::info!(
                        query = %body.query_text,
                        project_results = all_results.len(),
                        project_tags = ?tags,
                        discarded,
                        "should_recall_phase1_project"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "pipeline_search_project_failed");
                }
            }
        }
    }

    // Phase 2: Global fallback — supplement if project results insufficient, or no project tags
    let need_global = all_results.len() < max_results;
    if need_global || (project_tags_slice.is_none() || project_tags_slice.is_none_or(|t| t.is_empty())) {
        let global_search_req = SearchRequest {
            query: denoised_query.clone(),
            query_vector: query_vector.clone(),
            tenant_id: auth.tenant_id.clone(),
            scope_filter: None,
            limit: Some(max_results * phase2_multiplier),
            min_score: Some(effective_min_score),
            include_trace: false,
            tags_filter: None,
            source_filter: None,
            agent_id_filter: body.agent_id.clone(),
            accessible_spaces: accessible_space_ids.clone(),
            conversation_context: body.conversation_context.clone(),
        };
        match pipeline.search(&global_search_req, Some(&overrides)).await {
            Ok(results) => {
                for d in results.discarded {
                    if seen_ids_for_discarded.insert(d.memory.id.clone()) {
                        all_discarded.push(d);
                    }
                }
                let discarded_count = all_discarded.len();
                let remaining = if need_global { max_results.saturating_sub(all_results.len()) } else { max_results };
                let mut global_count = 0;
                for r in results.results {
                    if seen_ids.insert(r.memory.id.clone()) {
                        all_results.push(r);
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
                    discarded = discarded_count,
                    "should_recall_phase2_global"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "pipeline_search_global_failed");
            }
        }
    }

    let results = all_results;

    let memories: Vec<MemoryWithScore> = results
        .into_iter()
        .map(|r| MemoryWithScore {
            memory: r.memory,
            score: r.score,
            refine_relevance: r.refine_relevance,
            refine_reasoning: r.refine_reasoning,
        })
        .collect();

    let discarded_items: Vec<DiscardedItem> = all_discarded
        .iter()
        .map(|d| {
            let content = if d.memory.content.chars().count() > 200 {
                let end = d.memory.content.char_indices()
                    .nth(200).map(|(i, _)| i)
                    .unwrap_or(d.memory.content.len());
                format!("{}…", &d.memory.content[..end])
            } else {
                d.memory.content.clone()
            };
            DiscardedItem {
                memory_id: d.memory.id.clone(),
                content,
                score: d.score,
                refine_relevance: d.refine_relevance.clone(),
                refine_reasoning: d.refine_reasoning.clone(),
            }
        })
        .collect();

    tracing::info!(query = %body.query_text, result_count = memories.len(), discarded_count = discarded_items.len(), should_recall = !memories.is_empty(), "should_recall_result");

    let confidence = if memories.is_empty() {
        0.0
    } else {
        memories.iter().map(|m| m.score).sum::<f32>() / memories.len() as f32
    };
    let max_score = memories.iter().map(|m| m.score).fold(0.0_f32, f32::max);

    // 质量门槛：用max_score而非平均score，避免"一条高分拉高平均"的问题
    // quality_gate由LLM判断结果动态决定：yes=0.40, 信号词=0.50, 无信号=0.60
    if memories.is_empty() || max_score <= quality_gate {
        let reason = if memories.is_empty() {
            "no_relevant_memories".to_string()
        } else {
            tracing::info!(query = %body.query_text, confidence, max_score, quality_gate, "recall_below_quality_gate");
            "below_quality_gate".to_string()
        };
        return Ok(Json(ShouldRecallResponse {
            should_recall: false,
            reason: Some(reason),
            memories: None,
            discarded: if discarded_items.is_empty() { None } else { Some(discarded_items) },
            confidence: Some(confidence),
            similarity_score,
            clustered: None,
        }));
    }

    let clustered = {
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
    };

    let recalled_ids: Vec<String> = memories.iter().map(|r| r.memory.id.clone()).collect();
    if !recalled_ids.is_empty() {
        if let Err(e) = store.batch_bump_access_count(&recalled_ids).await {
            tracing::warn!(error = %e, "failed_to_batch_bump_access_count_after_recall");
        }
    }

    Ok(Json(ShouldRecallResponse {
        should_recall: true,
        reason: None,
        memories: Some(memories),
        discarded: if discarded_items.is_empty() { None } else { Some(discarded_items) },
        confidence: Some(confidence),
        similarity_score,
        clustered,
    }))
}

pub async fn list_session_groups(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<ListSessionGroupsQuery>,
) -> Result<Json<ListSessionGroupsResponse>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let raw_groups = store
        .list_session_groups(&auth.tenant_id)
        .await?;

    let total_count = raw_groups.len();
    let groups: Vec<SessionGroup> = raw_groups
        .into_iter()
        .skip(params.offset)
        .take(params.limit)
        .map(|g| SessionGroup {
            session_id: g.session_id,
            count: g.count,
            auto_count: g.auto_count,
            manual_count: g.manual_count,
            last_injected_at: g.last_injected_at,
            latest_query: g.latest_query,
        })
        .collect();

    Ok(Json(ListSessionGroupsResponse {
        groups,
        total_count,
        limit: params.limit,
        offset: params.offset,
    }))
}

pub async fn delete_session_recalls_by_session(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    if session_id.trim().is_empty() {
        return Err(OmemError::Validation("session_id cannot be empty".into()));
    }
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    store.delete_recall_events_by_session(&auth.tenant_id, &session_id).await?;

    Ok(Json(serde_json::json!({"success": true})))
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

static DENOISE_PATTERNS: LazyLock<(regex::Regex, regex::Regex, regex::Regex)> = LazyLock::new(|| {
    (
        regex::Regex::new(r"<[^>]+>").unwrap_or_else(|_| regex::Regex::new("").unwrap()),
        regex::Regex::new(r"```[\s\S]*?```").unwrap_or_else(|_| regex::Regex::new("").unwrap()),
        regex::Regex::new(r"\s+").unwrap_or_else(|_| regex::Regex::new("").unwrap()),
    )
});

fn denoise_for_recall(text: &str) -> String {
    let max_chars = 200;
    let mut cleaned = text.to_string();

    cleaned = DENOISE_PATTERNS.0.replace_all(&cleaned, "").to_string();
    cleaned = DENOISE_PATTERNS.1.replace_all(&cleaned, "").to_string();
    cleaned = DENOISE_PATTERNS.2.replace_all(cleaned.trim(), " ").to_string();

    if cleaned.chars().count() > max_chars {
        let truncated: String = cleaned.chars().take(max_chars).collect();
        format!("{}...(truncated)", truncated)
    } else {
        cleaned
    }
}

/// 检查query是否包含高信号关键词（低成本，无需embedding）
/// 仅检测明确的"引用历史"信号，避免编程通用词(config/key/token)误触发
/// 安全阀设计：只在LLM判断no但query明确引用历史时，给予一次严格搜索的机会
fn has_recall_signals(query: &str) -> bool {
    static SIGNAL_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        let patterns = [
            // 明确的历史/时间引用（中文）—— 最强信号，表明用户在回忆过去的交互
            r"(?:之前|上次|以前|过去|曾经|记得|忘了|忘记|那个方案|之前的|上次说的|之前做的|上次的)",
            // 密钥类（中文，编程通用词中极少自然出现）
            r"(?:密码|密钥|口令)",
            // 明确的历史引用（英文）
            r"(?i)(?:remember|recall|previously|last\s+time|earlier|that\s+time|before\s+this|the\s+other\s+day)",
        ];
        patterns.iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect()
    });

    for pattern in SIGNAL_PATTERNS.iter() {
        if pattern.is_match(query) {
            return true;
        }
    }
    false
}

#[derive(Deserialize)]
pub struct ListRecallEventsQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub session_id: Option<String>,
    pub expand: Option<String>,
}

#[derive(Serialize)]
pub struct RecallEventWithItems {
    #[serde(flatten)]
    pub event: RecallEvent,
    pub items: Vec<RecallItem>,
}

#[derive(Serialize)]
pub struct ListRecallEventsResponse {
    pub events: Vec<RecallEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_items: Option<Vec<RecallEventWithItems>>,
    pub limit: usize,
    pub offset: usize,
}

pub async fn list_recall_events(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<ListRecallEventsQuery>,
) -> Result<Json<ListRecallEventsResponse>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let events = store
        .list_recall_events(
            &auth.tenant_id,
            params.session_id.as_deref(),
            params.limit,
            params.offset,
        )
        .await?;

    let event_items = if params.expand.as_deref() == Some("items") && !events.is_empty() {
        let event_ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        let items_map = store
            .batch_list_recall_items_by_events(&auth.tenant_id, &event_ids)
            .await
            .unwrap_or_default();
        let mut result = Vec::with_capacity(events.len());
        for event in &events {
            let items = items_map.get(&event.id).cloned().unwrap_or_default();
            result.push(RecallEventWithItems {
                event: event.clone(),
                items,
            });
        }
        Some(result)
    } else {
        None
    };

    Ok(Json(ListRecallEventsResponse {
        events,
        event_items,
        limit: params.limit,
        offset: params.offset,
    }))
}

pub async fn list_recall_event_items(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(event_id): Path<String>,
) -> Result<Json<Vec<RecallItem>>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    let items = store
        .list_recall_items_by_event(&auth.tenant_id, &event_id)
        .await?;
    Ok(Json(items))
}

#[derive(Deserialize)]
pub struct UpdateProfileInjectedBody {
    pub profile_injected: bool,
    pub profile_content: Option<String>,
}

pub async fn update_recall_event_profile(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(event_id): Path<String>,
    Json(body): Json<UpdateProfileInjectedBody>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;
    store
        .update_recall_event_profile(&event_id, body.profile_injected, body.profile_content.as_deref())
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct CreateRecallEventBody {
    pub session_id: String,
    pub recall_type: Option<String>,
    pub query_text: String,
    pub max_score: f32,
    pub llm_confidence: f32,
    pub profile_injected: bool,
    pub kept_count: u32,
    pub discarded_count: u32,
    pub injected_count: Option<u32>,
    pub profile_content: Option<String>,
    pub injected_content: Option<String>,
    pub items: Option<Vec<CreateRecallEventItem>>,
}

#[derive(Deserialize)]
pub struct CreateRecallEventItem {
    pub memory_id: String,
    pub score: f32,
    pub refine_relevance: Option<String>,
    pub refine_reasoning: Option<String>,
    pub is_kept: bool,
}

pub async fn create_recall_event(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<CreateRecallEventBody>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let event_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let recall_type = body.recall_type.unwrap_or_else(|| "auto".to_string());

    let event = RecallEvent {
        id: event_id.clone(),
        session_id: body.session_id.clone(),
        recall_type,
        query_text: body.query_text,
        max_score: body.max_score,
        llm_confidence: body.llm_confidence,
        profile_injected: body.profile_injected,
        kept_count: body.kept_count,
        discarded_count: body.discarded_count,
        injected_count: body.injected_count.unwrap_or(0),
        profile_content: body.profile_content,
        injected_content: body.injected_content,
        tenant_id: auth.tenant_id.clone(),
        created_at: now.clone(),
    };

    store.create_recall_event(&event).await?;

    if let Some(items) = body.items {
        if !items.is_empty() {
            let recall_items: Vec<crate::store::lancedb::RecallItem> = items
                .into_iter()
                .map(|item| crate::store::lancedb::RecallItem {
                    id: uuid::Uuid::new_v4().to_string(),
                    event_id: event_id.clone(),
                    memory_id: item.memory_id,
                    score: item.score,
                    refine_relevance: item.refine_relevance,
                    refine_reasoning: item.refine_reasoning,
                    is_kept: item.is_kept,
                    tenant_id: auth.tenant_id.clone(),
                    created_at: now.clone(),
                })
                .collect();
            store.batch_create_recall_items(&recall_items).await?;
        }
    }

    Ok(Json(serde_json::json!({ "ok": true, "event_id": event_id })))
}
