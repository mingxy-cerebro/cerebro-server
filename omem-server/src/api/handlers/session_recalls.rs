use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::api::server::{personal_space_id, AppState};
use crate::domain::error::OmemError;
use crate::domain::memory::{sanitize_project_path, Memory};
use crate::domain::tenant::AuthInfo;
use crate::store::lancedb::{RecallEvent, RecallItem};
use crate::retrieve::pipeline::{RetrievalPipeline, SearchOverrides, SearchRequest, SearchResult};

type RecallTimeMap = HashMap<String, chrono::DateTime<chrono::Utc>>;
static LAST_RECALL_TIME: LazyLock<Arc<Mutex<RecallTimeMap>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalStrength {
    None,
    Weak,
    Medium,
    Strong,
}

const SHOULD_RECALL_SYSTEM_PROMPT: &str = r#"判断用户问题是否需要从知识库检索历史记忆，并提取检索关键词。

知识库内容：过往笔记、项目经验、技术决策、偏好设置、配置信息等。

需要召回的场景（满足任一即可）：
- 引用过去的事件、决策、方案（"之前"、"上次"、"we decided"）
- 询问个人偏好、习惯、工具选择
- 讨论特定项目/模块的设计或实现（可能涉及历史决策）
- 提到配置、环境、部署等（可能需要历史配置记录）
- 遇到问题寻求解决（可能之前有类似经验）

不需要召回的场景（仅在以下情况）：
- 纯通用知识问答（"什么是REST"、"冒泡排序怎么写"）
- 简单计算或数学推导
- 与用户个人历史完全无关的新话题闲聊

核心原则：宁可多召回。

以JSON格式回复：{"should_recall": true/false, "keywords": ["关键词1", "关键词2"]}
keywords为2-5个代表检索意图的核心词，去掉语气词和连接词。"#;

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
    pub project_path: Option<String>,
    #[serde(default)]
    pub skip_llm_gate: Option<bool>,
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

    let sanitized_project_path = match body.project_path.as_deref() {
        Some(pp) if !pp.is_empty() => Some(sanitize_project_path(pp).map_err(|e| {
            OmemError::Validation(format!("invalid project_path: {e}"))
        })?),
        _ => None,
    };

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

    let gate_system_prompt = SHOULD_RECALL_SYSTEM_PROMPT;
    let gate_user_prompt = format!(
        "用户问题：{}\n\n判断是否需要从知识库检索历史记忆，并提取2-5个最能代表检索意图的核心关键词（去掉语气词和连接词）。\n只回答JSON格式：{{\"should_recall\": true/false, \"keywords\": [\"关键词1\", \"关键词2\"]}}",
        denoised_query
    );

    #[derive(serde::Deserialize)]
    struct GateResponse {
        should_recall: bool,
        #[serde(default)]
        keywords: Vec<String>,
    }

    let skip_llm = body.skip_llm_gate.unwrap_or(false);
    let (llm_yes, llm_keywords) = if skip_llm {
        tracing::info!(query = %body.query_text, reason = "skipped_llm_gate", "recall_llm_skipped");
        (true, Vec::<String>::new())
    } else {
        match crate::llm::complete_json::<GateResponse>(
            state.recall_llm.as_ref(),
            gate_system_prompt,
            &gate_user_prompt,
        ).await {
            Ok(gate) => {
                tracing::info!(
                    query = %body.query_text,
                    should_recall = gate.should_recall,
                    keywords = ?gate.keywords,
                    "recall_llm_gate"
                );
                (gate.should_recall, gate.keywords)
            }
            Err(e) => {
                tracing::warn!(query = %body.query_text, error = %e, "recall_llm_error_fallback");
                (true, Vec::<String>::new())
            }
        }
    };

    let search_query = if llm_keywords.is_empty() {
        denoised_query.clone()
    } else {
        let keyword_part = llm_keywords.join(" ");
        if keyword_part.chars().count() < denoised_query.chars().count() / 2 {
            denoised_query.clone()
        } else {
            keyword_part
        }
    };
    tracing::info!(search_query = %search_query, source = if llm_keywords.is_empty() { "denoised" } else { "keywords" }, "recall_search_query");

    let vectors = state
        .embed
        .embed(std::slice::from_ref(&search_query))
        .await
        .map_err(|e| OmemError::Embedding(format!("failed to embed query: {e}")))?;
    let query_vector = vectors.into_iter().next();

    let store = state
        .store_manager
        .get_store(&personal_space_id(&auth.tenant_id))
        .await?;

    let mut max_results = body.max_results.unwrap_or(5);
    if max_results == 0 {
        max_results = 5;
    }

    let signal_level = has_recall_signals(&denoised_query);
    let effective_min_score = body.similarity_threshold.unwrap_or_else(|| {
        if llm_yes || signal_level == SignalStrength::Strong {
            0.35
        } else {
            match signal_level {
                SignalStrength::Medium => 0.40,
                SignalStrength::Weak => 0.45,
                SignalStrength::None => 0.50,
                SignalStrength::Strong => unreachable!(),
            }
        }
    });
    let quality_gate = if llm_yes || signal_level == SignalStrength::Strong {
        0.35
    } else {
        match signal_level {
            SignalStrength::Medium => 0.40,
            SignalStrength::Weak => 0.42,
            SignalStrength::None => 0.48,
            SignalStrength::Strong => unreachable!(),
        }
    };

    tracing::info!(
        query = %body.query_text,
        signal_level = ?signal_level,
        llm_yes,
        effective_min_score,
        quality_gate,
        "should_recall_thresholds"
    );

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
            query: search_query.clone(),
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
                project_path_filter: sanitized_project_path.clone(),
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
            query: search_query.clone(),
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
                project_path_filter: sanitized_project_path.clone(),
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
    // quality_gate由LLM判断+SignalStrength四档动态决定：Strong/LLM=yes=0.35, Medium=0.40, Weak=0.42, None=0.48
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
        }));
    }

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

/// 检查query中的记忆召回信号强度（低成本，无需embedding）
/// 四档：Strong（明确历史引用）> Medium（偏好/项目/配置）> Weak（编程求助隐含上下文）> None
/// 重要：不包含编程通用词（config/key/data/function/variable）作为独立触发词
fn has_recall_signals(query: &str) -> SignalStrength {
    // STRONG：明确的历史引用信号
    static STRONG_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        let patterns = [
            // 中文历史引用
            r"(?:之前|上次|以前|过去|曾经|记得|忘了|忘记|回忆|想起|提到过|说过|讨论过|聊过)",
            // 中文指代
            r"(?:那个方案|之前的|上次说的|之前做的|上次的|那个问题|那个项目|那个决定|之前讨论|之前决定|之前选的|之前定的)",
            // 中文密钥
            r"(?:密码|密钥|口令|凭证)",
            // 中文检索意图
            r"(?:查一下|找一下|翻一下|看一下|搜一下|调出来).{0,4}(?:之前的|上次的|以前的|历史的|过去的|记录|笔记|文档|方案)",
            // 英文历史
            r"(?i)(?:remember|recall|previously|last\s+time|earlier|that\s+time|before\s+this|the\s+other\s+day|we\s+discussed|we\s+talked|we\s+decided|as\s+we\s+agreed|from\s+before)",
            // 英文指代
            r"(?i)(?:that\s+(?:project|decision|approach|solution|issue|feature|module|component)|the\s+one\s+we|what\s+did\s+we|how\s+did\s+we|what\s+was\s+the)",
            // 英文密钥
            r"(?i)\b(?:password|secret|credential|api\s*key|access\s*token|secret\s*key)\b",
        ];
        patterns.iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect()
    });

    // MEDIUM：偏好/项目/配置
    static MEDIUM_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        let patterns = [
            // 中文偏好
            r"(?:我喜欢|我习惯|我的偏好|我通常|我一般|我喜欢用|我倾向于|我的风格|我的习惯|偏好)",
            // 中文项目
            r"(?:这个项目|这个模块|这个组件|这个功能|这个服务|这个系统|这个架构|团队|同事|协作|代码规范|技术栈|项目结构)",
            // 中文配置
            r"(?:环境变量|配置文件|部署|上线|服务器|数据库连接|端口号|域名|证书)",
            // 中文决策
            r"(?:方案选择|技术选型|为什么用|为什么选|选型|决策|规范|标准|约定)",
            // 英文偏好
            r"(?i)(?:i\s+(?:prefer|like|usually|typically|normally|always|tend\s+to)|my\s+(?:preference|style|habit|setup))",
            // 英文项目
            r"(?i)(?:in\s+this\s+(?:project|repo|codebase|team)|our\s+(?:project|team|codebase|architecture|stack|standard|convention)|how\s+(?:do\s+we|does\s+our))",
            // 英文配置
            r"(?i)(?:deploy|deployment|production|staging|environment\s+var|infrastructure|ci.?cd|pipeline)",
        ];
        patterns.iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect()
    });

    // WEAK：编程求助中的隐含上下文需求
    static WEAK_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        let patterns = [
            // 中文求助+领域
            r"(?:怎么实现|如何实现|怎么处理|如何处理|怎么解决|如何解决).{0,10}(?:功能|模块|接口|方案|架构|系统|组件|服务|认证|授权|缓存|队列)",
            // 中文错误
            r"(?:遇到了|报错了|出bug|崩溃|异常).{0,10}(?:问题|错误|bug|异常|崩溃)",
            // 中文改造
            r"(?:优化|重构|迁移|升级|改造|适配).{0,10}(?:方案|架构|系统|模块|组件|服务)",
            // 英文求助
            r"(?i)(?:how\s+(?:should|do|can|might|would)\s+we\s+(?:implement|handle|solve|approach|deal\s+with))",
            // 英文改造
            r"(?i)(?:refactor|migrat|upgrad|rewrit|redesign|port)\s+(?:this|the|our)",
        ];
        patterns.iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect()
    });

    for pattern in STRONG_PATTERNS.iter() {
        if pattern.is_match(query) {
            return SignalStrength::Strong;
        }
    }
    for pattern in MEDIUM_PATTERNS.iter() {
        if pattern.is_match(query) {
            return SignalStrength::Medium;
        }
    }
    for pattern in WEAK_PATTERNS.iter() {
        if pattern.is_match(query) {
            return SignalStrength::Weak;
        }
    }
    SignalStrength::None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_recall_request_deserializes_project_path() {
        let json = r#"{
            "query_text": "test query",
            "session_id": "sess_123",
            "project_path": "/home/user/project"
        }"#;
        let req: ShouldRecallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.project_path.as_deref(), Some("/home/user/project"));
        assert_eq!(req.query_text, "test query");
        assert_eq!(req.session_id, "sess_123");
    }

    #[test]
    fn test_should_recall_request_project_path_optional() {
        let json = r#"{
            "query_text": "test query",
            "session_id": "sess_123"
        }"#;
        let req: ShouldRecallRequest = serde_json::from_str(json).unwrap();
        assert!(req.project_path.is_none());
    }

    #[test]
    fn test_should_recall_request_project_path_null() {
        let json = r#"{
            "query_text": "test query",
            "session_id": "sess_123",
            "project_path": null
        }"#;
        let req: ShouldRecallRequest = serde_json::from_str(json).unwrap();
        assert!(req.project_path.is_none());
    }

    #[test]
    fn test_should_recall_request_skip_llm_gate() {
        let json = r#"{
            "query_text": "test",
            "session_id": "s1",
            "skip_llm_gate": true
        }"#;
        let req: ShouldRecallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.skip_llm_gate, Some(true));
    }

    #[test]
    fn test_should_recall_request_skip_llm_gate_default() {
        let json = r#"{
            "query_text": "test",
            "session_id": "s1"
        }"#;
        let req: ShouldRecallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.skip_llm_gate, None);
    }

    // --- SignalStrength tests ---

    #[test]
    fn test_signal_strong_chinese_history() {
        assert_eq!(has_recall_signals("之前做的方案"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_strong_chinese_reference() {
        assert_eq!(has_recall_signals("上次说的那个问题"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_strong_chinese_secret() {
        assert_eq!(has_recall_signals("数据库密码是多少"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_strong_chinese_retrieval_intent() {
        assert_eq!(has_recall_signals("查一下之前的记录"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_strong_english_history() {
        assert_eq!(has_recall_signals("remember what we discussed"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_strong_english_reference() {
        assert_eq!(has_recall_signals("that project we worked on"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_strong_english_secret() {
        assert_eq!(has_recall_signals("what is the api key"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_medium_chinese_preference() {
        assert_eq!(has_recall_signals("我喜欢用Rust写后端"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_medium_chinese_project() {
        assert_eq!(has_recall_signals("这个项目的架构怎么样"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_medium_chinese_config() {
        assert_eq!(has_recall_signals("环境变量怎么配置"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_medium_chinese_decision() {
        assert_eq!(has_recall_signals("技术选型为什么用Go"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_medium_english_preference() {
        assert_eq!(has_recall_signals("I prefer dark mode"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_medium_english_project() {
        assert_eq!(has_recall_signals("in this project we use React"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_medium_english_config() {
        assert_eq!(has_recall_signals("deployment pipeline setup"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_weak_chinese_help() {
        assert_eq!(has_recall_signals("怎么实现用户认证功能"), SignalStrength::Weak);
    }

    #[test]
    fn test_signal_weak_chinese_error() {
        assert_eq!(has_recall_signals("遇到了一个bug问题"), SignalStrength::Weak);
    }

    #[test]
    fn test_signal_weak_chinese_refactor() {
        // "这个模块" matches Medium project pattern, Medium takes priority over Weak
        assert_eq!(has_recall_signals("优化这个模块的架构"), SignalStrength::Medium);
    }

    #[test]
    fn test_signal_weak_english_help() {
        assert_eq!(has_recall_signals("how should we implement the auth module"), SignalStrength::Weak);
    }

    #[test]
    fn test_signal_weak_english_refactor() {
        assert_eq!(has_recall_signals("refactor this component"), SignalStrength::Weak);
    }

    #[test]
    fn test_signal_none_generic_question() {
        assert_eq!(has_recall_signals("今天天气怎么样"), SignalStrength::None);
    }

    #[test]
    fn test_signal_none_sorting_algorithm() {
        assert_eq!(has_recall_signals("冒泡排序怎么写"), SignalStrength::None);
    }

    #[test]
    fn test_signal_none_rest_definition() {
        assert_eq!(has_recall_signals("什么是REST"), SignalStrength::None);
    }

    #[test]
    fn test_signal_none_general_coding_words() {
        // Programming generic words should NOT trigger Weak or above
        assert_eq!(has_recall_signals("config the data function with a variable"), SignalStrength::None);
    }

    #[test]
    fn test_signal_none_key_value_pair() {
        assert_eq!(has_recall_signals("create a key value store"), SignalStrength::None);
    }

    #[test]
    fn test_signal_strong_takes_priority_over_medium() {
        // "之前" is Strong, "这个项目" is Medium — Strong should win
        assert_eq!(has_recall_signals("之前这个项目的方案"), SignalStrength::Strong);
    }

    #[test]
    fn test_signal_medium_takes_priority_over_weak() {
        // "这个模块" is Medium, but "怎么实现" + domain would be Weak — Medium wins
        assert_eq!(has_recall_signals("这个模块怎么处理"), SignalStrength::Medium);
    }

    #[test]
    fn test_config_should_recall_min_interval_default() {
        let config = crate::config::OmemConfig::default();
        assert_eq!(config.should_recall_min_interval_secs, 30);
    }
}
