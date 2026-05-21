use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use std::time::Duration;

use tracing::warn;

use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::lifecycle::decay::{DecayConfig, DecayEngine};
use crate::store::lancedb::LanceStore;

use super::reranker::Reranker;
use super::trace::{RetrievalTrace, StageTrace};

#[derive(Clone, Debug, Default)]
pub struct SearchOverrides {
    pub fetch_multiplier: Option<usize>,
    pub topk_cap_multiplier: Option<usize>,
    pub mmr_jaccard_threshold: Option<f32>,
    pub mmr_penalty_factor: Option<f32>,
    pub llm_max_eval: Option<usize>,
    pub refine_strategy: Option<String>,
    pub refine_timeout_secs: Option<u64>,
}

pub struct SearchRequest {
    pub query: String,
    pub query_vector: Option<Vec<f32>>,
    pub tenant_id: String,
    pub scope_filter: Option<String>,
    pub limit: Option<usize>,
    pub min_score: Option<f32>,
    pub include_trace: bool,
    pub tags_filter: Option<Vec<String>>,
    pub source_filter: Option<String>,
    pub agent_id_filter: Option<String>,
    pub accessible_spaces: Vec<String>,
    pub conversation_context: Option<Vec<String>>,
    pub project_path_filter: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub memory: Memory,
    pub score: f32,
    pub refine_relevance: Option<String>,
    pub refine_reasoning: Option<String>,
}

pub struct SearchResults {
    pub results: Vec<SearchResult>,
    pub discarded: Vec<SearchResult>,
    pub trace: RetrievalTrace,
}

pub struct RetrievalPipeline {
    store: Arc<LanceStore>,
    reranker: Option<Reranker>,
    decay_engine: DecayEngine,
    pinned_boost: f32,
    rrf_k: f32,
    vector_weight: f32,
    bm25_weight: f32,
    tag_weight: f32,
    min_score: f32,
    hard_cutoff: f32,
    default_limit: usize,
    llm: Option<Arc<dyn crate::llm::LlmService>>,
}

struct FusionEntry {
    memory: Memory,
    rrf_score: f32,
    bm25_score: f32,
    pre_rerank_score: f32,
}

const MAX_RETRIES: u32 = 2;
const RETRY_DELAY_MS: u64 = 500;

impl RetrievalPipeline {
    pub fn new(store: Arc<LanceStore>) -> Self {
        Self {
            store,
            reranker: None,
            decay_engine: DecayEngine::new(DecayConfig::default()),
            pinned_boost: 1.5,
            rrf_k: 60.0,
            vector_weight: 0.7,
            bm25_weight: 0.3,
            tag_weight: 0.2,
            min_score: 0.15,
            hard_cutoff: 0.005,
            default_limit: 20,
            llm: None,
        }
    }

    pub fn with_pinned_boost(mut self, boost: f32) -> Self {
        self.pinned_boost = boost;
        self
    }

    pub fn with_rrf_k(mut self, k: f32) -> Self {
        self.rrf_k = k;
        self
    }

    pub fn with_weights(mut self, vector: f32, bm25: f32) -> Self {
        self.vector_weight = vector;
        self.bm25_weight = bm25;
        self
    }

    pub fn with_min_score(mut self, score: f32) -> Self {
        self.min_score = score;
        self
    }

    pub fn with_hard_cutoff(mut self, cutoff: f32) -> Self {
        self.hard_cutoff = cutoff;
        self
    }

    pub fn with_decay_config(mut self, config: DecayConfig) -> Self {
        self.decay_engine = DecayEngine::new(config);
        self
    }

    pub fn with_default_limit(mut self, limit: usize) -> Self {
        self.default_limit = limit;
        self
    }

    pub fn with_reranker(mut self, reranker: Reranker) -> Self {
        self.reranker = Some(reranker);
        self
    }

    pub fn with_decay_engine(mut self, engine: DecayEngine) -> Self {
        self.decay_engine = engine;
        self
    }

    pub fn with_tag_weight(mut self, weight: f32) -> Self {
        self.tag_weight = weight;
        self
    }

    pub fn with_llm(mut self, llm: Arc<dyn crate::llm::LlmService>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub async fn search(
        &self,
        request: &SearchRequest,
        overrides: Option<&SearchOverrides>,
    ) -> Result<SearchResults, OmemError> {
        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            match self.search_inner(request, overrides).await {
                Ok(results) => return Ok(results),
                Err(e) => {
                    let is_retryable = matches!(e, OmemError::Storage(_));
                    if !is_retryable || attempt == MAX_RETRIES {
                        return Err(e);
                    }
                    tracing::warn!(
                        attempt = attempt + 1,
                        max_retries = MAX_RETRIES,
                        error = %e,
                        "search_retry_storage_error"
                    );
                    tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS * (attempt as u64 + 1))).await;
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap())
    }

    async fn search_inner(
        &self,
        request: &SearchRequest,
        overrides: Option<&SearchOverrides>,
    ) -> Result<SearchResults, OmemError> {
        let pipeline_start = Instant::now();
        let mut trace = RetrievalTrace::new();
        let limit = request.limit.unwrap_or(self.default_limit);
        let min_score = request.min_score.unwrap_or(self.min_score);

        let fetch_multiplier = overrides.and_then(|o| o.fetch_multiplier).unwrap_or(3);
        let topk_cap_multiplier = overrides.and_then(|o| o.topk_cap_multiplier).unwrap_or(2);
        let mmr_jaccard_threshold = overrides.and_then(|o| o.mmr_jaccard_threshold).unwrap_or(0.85);
        let mmr_penalty_factor = overrides.and_then(|o| o.mmr_penalty_factor).unwrap_or(0.5);
        let llm_max_eval = overrides.and_then(|o| o.llm_max_eval).unwrap_or(15);
        let refine_strategy = overrides
            .and_then(|o| o.refine_strategy.clone())
            .unwrap_or_else(|| "balanced".to_string())
            .to_lowercase();
        let refine_timeout_secs = overrides.and_then(|o| o.refine_timeout_secs).unwrap_or(15);

        tracing::info!(
            fetch_multiplier,
            topk_cap_multiplier,
            mmr_jaccard_threshold,
            mmr_penalty_factor,
            llm_max_eval,
            refine_strategy = %refine_strategy,
            refine_timeout_secs,
            "recall_search_params"
        );

        let fetch_limit = limit * fetch_multiplier;

        let (candidates, stage1) = self.stage_parallel_search(request, fetch_limit).await?;
        trace.add_stage(stage1);

        if candidates.vector.is_empty() && candidates.bm25.is_empty() {
            trace.finalize(0, pipeline_start.elapsed().as_millis() as u64);
            return Ok(SearchResults {
                results: Vec::new(),
                discarded: Vec::new(),
                trace,
            });
        }

        let (tag_ranks, stage_tag) =
            self.stage_tag_boost(&candidates, request.tags_filter.as_deref());
        trace.add_stage(stage_tag);

        let (fused, stage2) = self.stage_rrf_fusion(candidates, &tag_ranks);
        trace.add_stage(stage2);

        let (normalized_rrf, stage3) = Self::stage_rrf_normalize(fused);
        trace.add_stage(stage3);

        let (filtered, stage4) = Self::stage_min_score_filter(normalized_rrf, min_score);
        trace.add_stage(stage4);

        let (capped, stage5) = Self::stage_topk_cap(filtered, limit * topk_cap_multiplier);
        trace.add_stage(stage5);

        let (expanded, stage_expand) = self.stage_expand_relations(capped).await;
        trace.add_stage(stage_expand);

        let (reranked, stage6) = self
            .stage_cross_encoder_rerank(expanded, &request.query)
            .await;
        trace.add_stage(stage6);

        let (floored, stage7) = Self::stage_bm25_floor(reranked);
        trace.add_stage(stage7);

        let (decayed, stage8) = self.stage_decay_boost(floored);
        trace.add_stage(stage8);

        let (weighted, stage9) = Self::stage_importance_weight(decayed);
        trace.add_stage(stage9);

        let (normalized, stage10) = Self::stage_length_normalization(weighted);
        trace.add_stage(stage10);

        let (cutoff_results, stage11) = Self::stage_hard_cutoff(normalized, self.hard_cutoff);
        trace.add_stage(stage11);

        let (final_entries, stage12) = Self::stage_mmr_diversity(cutoff_results, limit, mmr_jaccard_threshold, mmr_penalty_factor);
        trace.add_stage(stage12);

        let results: Vec<SearchResult> = final_entries
            .into_iter()
            .map(|e| SearchResult {
                memory: e.memory,
                score: e.rrf_score,
                refine_relevance: None,
                refine_reasoning: None,
            })
            .collect();

        let (refined, discarded, stage_llm) = self
            .stage_llm_refine(
                results,
                &request.query,
                request.conversation_context.as_deref(),
                llm_max_eval,
                &refine_strategy,
                refine_timeout_secs,
            )
            .await;
        trace.add_stage(stage_llm);

        let count = refined.len();
        trace.finalize(count, pipeline_start.elapsed().as_millis() as u64);

        Ok(SearchResults {
            results: refined,
            discarded,
            trace,
        })
    }

    async fn stage_parallel_search(
        &self,
        request: &SearchRequest,
        fetch_limit: usize,
    ) -> Result<(ParallelResults, StageTrace), OmemError> {
        let stage_start = Instant::now();
        let scope = request.scope_filter.as_deref();
        let project_path = request.project_path_filter.as_deref();

        let visibility_filter = request
            .agent_id_filter
            .as_deref()
            .map(|agent_id| self.store.build_visibility_filter(agent_id, &request.accessible_spaces));
        let vis_ref = visibility_filter.as_deref();

        let vector_fut = async {
            if let Some(ref qv) = request.query_vector {
                self.store
                    .vector_search(qv, fetch_limit, 0.0, scope, vis_ref, None, None, project_path)
                    .await
            } else {
                Ok(Vec::new())
            }
        };

        let bm25_fut = async {
            self.store
                .fts_search(&request.query, fetch_limit, scope, vis_ref, None, project_path)
                .await
        };

        let (vector_res, bm25_res) = tokio::join!(vector_fut, bm25_fut);

        let vector_results = match vector_res {
            Ok(v) => v,
            Err(e) => {
                warn!("vector search failed, falling back to BM25-only: {e}");
                Vec::new()
            }
        };

        let bm25_results = match bm25_res {
            Ok(v) => v,
            Err(e) => {
                warn!("BM25 search failed, falling back to vector-only: {e}");
                Vec::new()
            }
        };

        let total_count = vector_results.len() + bm25_results.len();
        let score_range = Self::compute_score_range(&vector_results, &bm25_results);

        let stage = StageTrace {
            name: "parallel_search".to_string(),
            input_count: 0,
            output_count: total_count,
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        Ok((
            ParallelResults {
                vector: vector_results,
                bm25: bm25_results,
            },
            stage,
        ))
    }

    fn stage_tag_boost(
        &self,
        candidates: &ParallelResults,
        tags_filter: Option<&[String]>,
    ) -> (Vec<(String, usize)>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = candidates.vector.len() + candidates.bm25.len();

        let tags = match tags_filter {
            Some(t) if !t.is_empty() => t,
            _ => {
                let stage = StageTrace {
                    name: "tag_boost".to_string(),
                    input_count,
                    output_count: 0,
                    dropped_ids: Vec::new(),
                    score_range: None,
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                };
                return (Vec::new(), stage);
            }
        };

        let tag_set: HashSet<&str> = tags.iter().map(|s| s.as_str()).collect();

        let mut seen = HashSet::new();
        let mut scored: Vec<(String, usize)> = Vec::new();

        for (memory, _score) in candidates.vector.iter().chain(candidates.bm25.iter()) {
            if seen.insert(memory.id.clone()) {
                let overlap = memory
                    .tags
                    .iter()
                    .filter(|t| tag_set.contains(t.as_str()))
                    .count();
                if overlap > 0 {
                    scored.push((memory.id.clone(), overlap));
                }
            }
        }

        scored.sort_by(|a, b| b.1.cmp(&a.1));

        let ranked: Vec<(String, usize)> = scored
            .into_iter()
            .enumerate()
            .map(|(rank, (id, _overlap))| (id, rank))
            .collect();

        let output_count = ranked.len();
        let stage = StageTrace {
            name: "tag_boost".to_string(),
            input_count,
            output_count,
            dropped_ids: Vec::new(),
            score_range: None,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (ranked, stage)
    }

    fn stage_rrf_fusion(
        &self,
        candidates: ParallelResults,
        tag_ranks: &[(String, usize)],
    ) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = candidates.vector.len() + candidates.bm25.len() + tag_ranks.len();

        let total_w = self.vector_weight + self.bm25_weight + self.tag_weight;
        let vw = self.vector_weight / total_w;
        let bw = self.bm25_weight / total_w;
        let tw = self.tag_weight / total_w;

        let mut score_map: HashMap<String, FusionEntry> = HashMap::new();

        for (rank, (memory, _score)) in candidates.vector.into_iter().enumerate() {
            let rrf = vw / (self.rrf_k + (rank + 1) as f32);
            score_map
                .entry(memory.id.clone())
                .and_modify(|e| e.rrf_score += rrf)
                .or_insert(FusionEntry {
                    memory,
                    rrf_score: rrf,
                    bm25_score: 0.0,
                    pre_rerank_score: 0.0,
                });
        }

        for (rank, (memory, bm25_raw)) in candidates.bm25.into_iter().enumerate() {
            let rrf = bw / (self.rrf_k + (rank + 1) as f32);
            score_map
                .entry(memory.id.clone())
                .and_modify(|e| {
                    e.rrf_score += rrf;
                    e.bm25_score = bm25_raw;
                })
                .or_insert(FusionEntry {
                    memory,
                    rrf_score: rrf,
                    bm25_score: bm25_raw,
                    pre_rerank_score: 0.0,
                });
        }

        let mut fused: Vec<FusionEntry> = score_map.into_values().collect();

        let tag_map: HashMap<&str, usize> = tag_ranks
            .iter()
            .map(|(id, rank)| (id.as_str(), *rank))
            .collect();
        for entry in &mut fused {
            if let Some(&tag_rank) = tag_map.get(entry.memory.id.as_str()) {
                let rrf = tw / (self.rrf_k + (tag_rank + 1) as f32);
                entry.rrf_score += rrf;
            }
        }

        for entry in &mut fused {
            if entry.memory.memory_type.is_pinned() {
                entry.rrf_score *= self.pinned_boost;
            }
        }

        let score_range = fusion_score_range(&fused);

        let stage = StageTrace {
            name: "rrf_fusion".to_string(),
            input_count,
            output_count: fused.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (fused, stage)
    }

    /// Normalize RRF scores to [0, 1] range so downstream thresholds (min_score, hard_cutoff) work correctly.
    /// RRF raw scores are tiny (max ~0.033 for K=60 with 2 legs), but thresholds expect [0, 1].
    /// - Multiple results: min-max normalization (best=1.0, worst=0.0)
    fn stage_rrf_normalize(mut entries: Vec<FusionEntry>) -> (Vec<FusionEntry>, StageTrace) {
        const RRF_SCALE: f32 = 120.0;
        let stage_start = Instant::now();
        let input_count = entries.len();

        if entries.len() > 1 {
            let max_score = entries
                .iter()
                .map(|e| e.rrf_score)
                .fold(f32::NEG_INFINITY, f32::max);
            let min_score = entries
                .iter()
                .map(|e| e.rrf_score)
                .fold(f32::INFINITY, f32::min);
            let range = max_score - min_score;
            if range > 0.0 {
                for entry in &mut entries {
                    entry.rrf_score = (entry.rrf_score - min_score) / range;
                }
            } else if max_score > 0.0 {
                for entry in &mut entries {
                    entry.rrf_score = 1.0;
                }
            }
        } else if entries.len() == 1 {
            entries[0].rrf_score = (entries[0].rrf_score * RRF_SCALE).min(1.0);
        }

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "rrf_normalize".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    fn stage_min_score_filter(
        entries: Vec<FusionEntry>,
        min_score: f32,
    ) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        let mut kept = Vec::new();
        let mut dropped_ids = Vec::new();

        for entry in entries {
            if entry.rrf_score >= min_score {
                kept.push(entry);
            } else {
                dropped_ids.push(entry.memory.id);
            }
        }

        let score_range = fusion_score_range(&kept);

        let stage = StageTrace {
            name: "min_score_filter".to_string(),
            input_count,
            output_count: kept.len(),
            dropped_ids,
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (kept, stage)
    }

    fn stage_topk_cap(
        mut entries: Vec<FusionEntry>,
        limit: usize,
    ) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        entries.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let dropped_ids: Vec<String> = entries
            .iter()
            .skip(limit)
            .map(|e| e.memory.id.clone())
            .collect();

        entries.truncate(limit);

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "topk_cap".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids,
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    async fn stage_expand_relations(
        &self,
        mut candidates: Vec<FusionEntry>,
    ) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = candidates.len();

        let existing_ids: HashSet<String> =
            candidates.iter().map(|e| e.memory.id.clone()).collect();

        let mut relation_target_ids: Vec<String> = candidates
            .iter()
            .flat_map(|e| e.memory.relations.iter().map(|r| r.target_id.clone()))
            .filter(|id| !existing_ids.contains(id))
            .collect();

        relation_target_ids.sort();
        relation_target_ids.dedup();

        if relation_target_ids.is_empty() {
            let stage = StageTrace {
                name: "expand_relations".to_string(),
                input_count,
                output_count: candidates.len(),
                dropped_ids: Vec::new(),
                score_range: fusion_score_range(&candidates),
                duration_ms: stage_start.elapsed().as_millis() as u64,
            };
            return (candidates, stage);
        }

        let min_score = candidates
            .iter()
            .map(|e| e.rrf_score)
            .fold(f32::INFINITY, f32::min);
        let expanded_score = min_score * 0.8;

        let fetched = match self.store.get_memories_by_ids(&relation_target_ids).await {
            Ok(m) => m,
            Err(e) => {
                warn!("expand_relations fetch failed: {e}");
                let stage = StageTrace {
                    name: "expand_relations".to_string(),
                    input_count,
                    output_count: candidates.len(),
                    dropped_ids: Vec::new(),
                    score_range: fusion_score_range(&candidates),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                };
                return (candidates, stage);
            }
        };

        let mut added = 0usize;
        for memory in fetched {
            if added >= 20 {
                break;
            }
            if existing_ids.contains(&memory.id) {
                continue;
            }
            candidates.push(FusionEntry {
                memory,
                rrf_score: expanded_score,
                bm25_score: 0.0,
                pre_rerank_score: 0.0,
            });
            added += 1;
        }

        let score_range = fusion_score_range(&candidates);
        let stage = StageTrace {
            name: "expand_relations".to_string(),
            input_count,
            output_count: candidates.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (candidates, stage)
    }

    async fn stage_cross_encoder_rerank(
        &self,
        mut entries: Vec<FusionEntry>,
        query: &str,
    ) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        for e in &mut entries {
            e.pre_rerank_score = e.rrf_score;
        }

        if let Some(ref reranker) = self.reranker {
            let docs: Vec<&str> = entries.iter().map(|e| e.memory.content.as_str()).collect();
            match reranker.rerank(query, &docs).await {
                Ok(scores) => {
                    for (entry, &rerank_score) in entries.iter_mut().zip(scores.iter()) {
                        entry.rrf_score = rerank_score * 0.6 + entry.pre_rerank_score * 0.4;
                    }
                }
                Err(e) => {
                    warn!("reranker failed, keeping original scores: {e}");
                }
            }
        }

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "cross_encoder_rerank".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    fn stage_bm25_floor(mut entries: Vec<FusionEntry>) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        for entry in &mut entries {
            if entry.bm25_score >= 0.75 {
                let floor = entry.pre_rerank_score * 0.95;
                if entry.rrf_score < floor {
                    entry.rrf_score = floor;
                }
            }
        }

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "bm25_floor".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    fn stage_decay_boost(&self, mut entries: Vec<FusionEntry>) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        for entry in &mut entries {
            entry.rrf_score = self
                .decay_engine
                .apply_search_boost(entry.rrf_score, &entry.memory);
        }

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "decay_boost".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    fn stage_importance_weight(mut entries: Vec<FusionEntry>) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        for entry in &mut entries {
            entry.rrf_score *= 0.7 + 0.3 * entry.memory.importance;
        }

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "importance_weight".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    fn stage_length_normalization(mut entries: Vec<FusionEntry>) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        for entry in &mut entries {
            // Use l0_abstract length for normalization (content is often raw conversation)
            let text_for_len = if entry.memory.l0_abstract.is_empty() {
                &entry.memory.content
            } else {
                &entry.memory.l0_abstract
            };
            let len_ratio = text_for_len.len() as f32 / 500.0;
            let log_val = if len_ratio > 0.0 {
                len_ratio.log2()
            } else {
                0.0
            };
            let denominator = (1.0 + log_val).max(1.0);
            entry.rrf_score /= denominator;
        }

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "length_normalization".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids: Vec::new(),
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    fn stage_hard_cutoff(
        entries: Vec<FusionEntry>,
        threshold: f32,
    ) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        let mut kept = Vec::new();
        let mut dropped_ids = Vec::new();

        for entry in entries {
            if entry.rrf_score >= threshold {
                kept.push(entry);
            } else {
                dropped_ids.push(entry.memory.id);
            }
        }

        let score_range = fusion_score_range(&kept);

        let stage = StageTrace {
            name: "hard_cutoff".to_string(),
            input_count,
            output_count: kept.len(),
            dropped_ids,
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (kept, stage)
    }

    fn stage_mmr_diversity(
        mut entries: Vec<FusionEntry>,
        limit: usize,
        jaccard_threshold: f32,
        penalty_factor: f32,
    ) -> (Vec<FusionEntry>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = entries.len();

        entries.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for i in 1..entries.len() {
            let mut too_similar = false;
            for j in 0..i {
                if content_jaccard(&entries[j].memory.content, &entries[i].memory.content) > jaccard_threshold {
                    too_similar = true;
                    break;
                }
            }
            if too_similar {
                entries[i].rrf_score *= penalty_factor;
            }
        }

        entries.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let dropped_ids: Vec<String> = entries
            .iter()
            .skip(limit)
            .map(|e| e.memory.id.clone())
            .collect();
        entries.truncate(limit);

        let score_range = fusion_score_range(&entries);

        let stage = StageTrace {
            name: "mmr_diversity".to_string(),
            input_count,
            output_count: entries.len(),
            dropped_ids,
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (entries, stage)
    }

    async fn stage_llm_refine(
        &self,
        candidates: Vec<SearchResult>,
        query: &str,
        conversation_context: Option<&[String]>,
        max_eval: usize,
        refine_strategy: &str,
        timeout_secs: u64,
    ) -> (Vec<SearchResult>, Vec<SearchResult>, StageTrace) {
        let stage_start = Instant::now();
        let input_count = candidates.len();

        // loose strategy: skip LLM entirely, return all candidates
        if refine_strategy == "loose" {
            tracing::info!(
                strategy = "loose",
                input = input_count,
                "llm_refine_skipped"
            );
            let stage = StageTrace {
                name: "llm_refine".to_string(),
                input_count,
                output_count: candidates.len(),
                dropped_ids: Vec::new(),
                score_range: search_result_score_range(&candidates),
                duration_ms: stage_start.elapsed().as_millis() as u64,
            };
            return (candidates, Vec::new(), stage);
        }

        let llm = match self.llm {
            Some(ref l) => l,
            None => {
                let stage = StageTrace {
                    name: "llm_refine".to_string(),
                    input_count,
                    output_count: candidates.len(),
                    dropped_ids: Vec::new(),
                    score_range: search_result_score_range(&candidates),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                };
                return (candidates, Vec::new(), stage);
            }
        };

        let eval_candidates: Vec<SearchResult> = candidates
            .iter()
            .take(max_eval)
            .cloned()
            .collect();

        let memories: Vec<(String, &str, Option<&str>)> = eval_candidates
            .iter()
            .map(|r| {
                (
                    r.memory.id.clone(),
                    r.memory.content.as_str(),
                    if r.memory.l1_overview.is_empty() {
                        None
                    } else {
                        Some(r.memory.l1_overview.as_str())
                    },
                )
            })
            .collect();

        let user_prompt = super::prompts::build_refine_user_prompt(
            &memories,
            query,
            conversation_context,
        );
        let system_prompt = super::prompts::REFINE_SYSTEM_PROMPT;

        let refine_result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            crate::llm::complete_json::<super::prompts::RefineResponse>(
                llm.as_ref(),
                system_prompt,
                &user_prompt,
            ),
        )
        .await;

        let refined = match refine_result {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => {
                warn!("LLM refine JSON parse error: {e}");
                let stage = StageTrace {
                    name: "llm_refine".to_string(),
                    input_count,
                    output_count: candidates.len(),
                    dropped_ids: Vec::new(),
                    score_range: search_result_score_range(&candidates),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                };
                return (candidates, Vec::new(), stage);
            }
            Err(_) => {
                warn!("LLM refine timed out ({}s)", timeout_secs);
                let stage = StageTrace {
                    name: "llm_refine".to_string(),
                    input_count,
                    output_count: candidates.len(),
                    dropped_ids: Vec::new(),
                    score_range: search_result_score_range(&candidates),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                };
                return (candidates, Vec::new(), stage);
            }
        };

        let eval_ids: HashSet<&str> = eval_candidates.iter().map(|r| r.memory.id.as_str()).collect();

        // Build relevance map: only the eval_ids entries have LLM verdicts
        let relevance_map: HashMap<&str, &str> = refined
            .items
            .iter()
            .map(|item| (item.id.as_str(), item.relevance.as_str()))
            .collect();

        // Log each memory's refine judgment so we can audit what LLM decided
        for item in &refined.items {
            tracing::info!(
                memory_id = %item.id,
                relevance = %item.relevance,
                reasoning = %item.reasoning,
                "llm_refine_judgment"
            );
        }

        let relevant_ids: HashSet<&str> = refined
            .items
            .iter()
            .filter(|item| item.relevance != "irrelevant")
            .map(|item| item.id.as_str())
            .collect();

        let mut kept: Vec<SearchResult> = candidates
            .into_iter()
            .filter(|r| {
                if eval_ids.contains(r.memory.id.as_str()) {
                    relevant_ids.contains(r.memory.id.as_str())
                } else {
                    true
                }
            })
            .collect();

        // Apply medium relevance downgrade based on refine_strategy
        for r in &mut kept {
            if let Some(&relevance) = relevance_map.get(r.memory.id.as_str()) {
                r.refine_relevance = Some(relevance.to_string());
                if let Some(item) = refined.items.iter().find(|i| i.id == r.memory.id) {
                    r.refine_reasoning = Some(item.reasoning.clone());
                }
                if relevance == "medium" {
                    if refine_strategy == "strict" {
                        // strict: treat medium same as irrelevant — discard
                        r.memory.content.clear();
                    }
                    // balanced: keep content intact, only mark refine_relevance
                }
            }
        }

        // strict mode: filter out cleared (medium-turned-irrelevant) entries
        if refine_strategy == "strict" {
            kept.retain(|r| !r.memory.content.is_empty());
        }

        let kept_ids: HashSet<&str> = kept.iter().map(|r| r.memory.id.as_str()).collect();
        let dropped_ids: Vec<String> = eval_candidates
            .iter()
            .filter(|r| !kept_ids.contains(r.memory.id.as_str()))
            .map(|r| r.memory.id.clone())
            .collect();
        let eval_count = eval_candidates.len();
        // In strict mode, medium entries are also discarded
        let is_strict = refine_strategy == "strict";
        let discard_ref_map: HashMap<&str, &str> = refined
            .items
            .iter()
            .filter(|i| i.relevance == "irrelevant" || (is_strict && i.relevance == "medium"))
            .map(|i| (i.id.as_str(), i.relevance.as_str()))
            .collect();
        let mut discarded: Vec<SearchResult> = eval_candidates
            .into_iter()
            .filter(|r| discard_ref_map.contains_key(r.memory.id.as_str()))
            .collect();
        for r in &mut discarded {
            if let Some(item) = refined.items.iter().find(|i| i.id == r.memory.id) {
                r.refine_relevance = Some(item.relevance.clone());
                r.refine_reasoning = Some(item.reasoning.clone());
            }
        }

        let output_count = kept.len();
        let score_range = search_result_score_range(&kept);

        let high_count = refined.items.iter().filter(|i| i.relevance == "high").count();
        let medium_count = refined.items.iter().filter(|i| i.relevance == "medium").count();
        let irrelevant_count = refined.items.iter().filter(|i| i.relevance == "irrelevant").count();
        tracing::info!(
            input = input_count,
            evaluated = eval_count,
            high = high_count,
            medium = medium_count,
            irrelevant = irrelevant_count,
            kept = output_count,
            dropped = dropped_ids.len(),
            "llm_refine_summary"
        );

        let stage = StageTrace {
            name: "llm_refine".to_string(),
            input_count,
            output_count,
            dropped_ids,
            score_range,
            duration_ms: stage_start.elapsed().as_millis() as u64,
        };

        (kept, discarded, stage)
    }

    fn compute_score_range(
        vec_results: &[(Memory, f32)],
        bm25_results: &[(Memory, f32)],
    ) -> Option<(f32, f32)> {
        let all_scores: Vec<f32> = vec_results
            .iter()
            .chain(bm25_results.iter())
            .map(|(_, s)| *s)
            .collect();

        if all_scores.is_empty() {
            return None;
        }
        let min = all_scores.iter().copied().fold(f32::INFINITY, f32::min);
        let max = all_scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        Some((min, max))
    }
}

struct ParallelResults {
    vector: Vec<(Memory, f32)>,
    bm25: Vec<(Memory, f32)>,
}

fn fusion_score_range(entries: &[FusionEntry]) -> Option<(f32, f32)> {
    if entries.is_empty() {
        return None;
    }
    let min = entries
        .iter()
        .map(|e| e.rrf_score)
        .fold(f32::INFINITY, f32::min);
    let max = entries
        .iter()
        .map(|e| e.rrf_score)
        .fold(f32::NEG_INFINITY, f32::max);
    Some((min, max))
}

fn search_result_score_range(results: &[SearchResult]) -> Option<(f32, f32)> {
    if results.is_empty() {
        return None;
    }
    let min = results
        .iter()
        .map(|r| r.score)
        .fold(f32::INFINITY, f32::min);
    let max = results
        .iter()
        .map(|r| r.score)
        .fold(f32::NEG_INFINITY, f32::max);
    Some((min, max))
}

fn content_jaccard(a: &str, b: &str) -> f32 {
    let a_ngrams = text_ngrams(a);
    let b_ngrams = text_ngrams(b);
    let union_count = a_ngrams.union(&b_ngrams).count();
    if union_count == 0 {
        return 1.0;
    }
    let inter_count = a_ngrams.intersection(&b_ngrams).count();
    inter_count as f32 / union_count as f32
}

/// Generate n-grams for text similarity comparison.
/// Uses character bigrams for CJK text (no spaces), whitespace-split words otherwise.
fn text_ngrams(text: &str) -> HashSet<String> {
    let has_cjk = text.chars().any(|c| {
        ('\u{4E00}'..='\u{9FFF}').contains(&c)  // CJK Unified Ideographs
            || ('\u{3400}'..='\u{4DBF}').contains(&c)  // CJK Extension A
            || ('\u{3040}'..='\u{30FF}').contains(&c)  // Hiragana + Katakana
    });

    if has_cjk {
        // Character bigrams for CJK text
        let chars: Vec<char> = text.chars().collect();
        if chars.len() < 2 {
            return HashSet::from([text.to_string()]);
        }
        (0..chars.len() - 1)
            .map(|i| format!("{}{}", chars[i], chars[i + 1]))
            .collect()
    } else {
        // Whitespace-split words for non-CJK text
        text.split_whitespace().map(|s| s.to_lowercase()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::Category;
    use crate::domain::types::MemoryType;
    use tempfile::TempDir;

    async fn setup() -> (Arc<LanceStore>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store = LanceStore::new(dir.path().to_str().expect("invalid path"))
            .await
            .expect("failed to create store");
        store.init_table().await.expect("failed to init table");
        (Arc::new(store), dir)
    }

    fn make_memory(tenant: &str, content: &str, mem_type: MemoryType) -> Memory {
        Memory::new(content, Category::new("preferences"), mem_type, tenant)
    }

    fn make_entry(content: &str, score: f32) -> FusionEntry {
        FusionEntry {
            memory: make_memory("t", content, MemoryType::Insight),
            rrf_score: score,
            bm25_score: 0.0,
            pre_rerank_score: 0.0,
        }
    }

    const DIM: usize = 1024;

    fn make_vector(primary: usize, value: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; DIM];
        v[primary] = value;
        v
    }

    #[tokio::test]
    async fn test_hybrid_search() {
        let (store, _dir) = setup().await;

        let m1 = make_memory(
            "t-001",
            "rust programming language is fast",
            MemoryType::Insight,
        );
        let m2 = make_memory("t-001", "python scripting language", MemoryType::Insight);
        let m3 = make_memory("t-001", "the weather is sunny today", MemoryType::Insight);

        let v1 = make_vector(0, 1.0);
        let v2 = make_vector(0, 0.8);
        let v3 = make_vector(1, 1.0);

        store.create(&m1, Some(&v1)).await.expect("create m1");
        store.create(&m2, Some(&v2)).await.expect("create m2");
        store.create(&m3, Some(&v3)).await.expect("create m3");

        store.create_fts_index().await.expect("create fts index");

        let pipeline = RetrievalPipeline::new(store)
            .with_min_score(0.0)
            .with_hard_cutoff(0.0);

        let request = SearchRequest {
            query: "rust programming".to_string(),
            query_vector: Some(make_vector(0, 1.0)),
            tenant_id: "t-001".to_string(),
            scope_filter: None,
            limit: Some(10),
            min_score: Some(0.0),
            include_trace: true,
            tags_filter: None,
            source_filter: None,
            agent_id_filter: None,
            accessible_spaces: Vec::new(),
            conversation_context: None,
            project_path_filter: None,
        };

        let results = pipeline.search(&request, None).await.expect("search should succeed");
        assert_eq!(results.results.len(), 2);
        assert!(results.trace.stages.len() >= 1);
    }

    #[tokio::test]
    async fn test_rrf_fusion_scoring() {
        let rrf_k = 60.0_f32;
        let vector_weight = 0.7_f32;
        let bm25_weight = 0.3_f32;

        let rank1_vector = vector_weight / (rrf_k + 1.0);
        let rank1_bm25 = bm25_weight / (rrf_k + 1.0);
        let rank2_vector = vector_weight / (rrf_k + 2.0);

        let combined = rank1_vector + rank1_bm25;
        assert!(
            combined > rank2_vector,
            "item in both legs should score higher than item in one leg"
        );

        let expected_combined = 1.0 / (rrf_k + 1.0);
        let diff = (combined - expected_combined).abs();
        assert!(
            diff < 1e-6,
            "combined should equal 1/(k+1) when weights sum to 1.0"
        );
    }

    #[tokio::test]
    async fn test_pinned_boost() {
        let (store, _dir) = setup().await;

        let m_pinned = make_memory("t-001", "important pinned fact", MemoryType::Pinned);
        let m_normal = make_memory("t-001", "important normal fact", MemoryType::Insight);

        let v = make_vector(0, 1.0);
        store
            .create(&m_pinned, Some(&v))
            .await
            .expect("create pinned");
        store
            .create(&m_normal, Some(&v))
            .await
            .expect("create normal");

        let pipeline = RetrievalPipeline::new(store)
            .with_min_score(0.0)
            .with_hard_cutoff(0.0);

        let request = SearchRequest {
            query: "important".to_string(),
            query_vector: Some(make_vector(0, 1.0)),
            tenant_id: "t-001".to_string(),
            scope_filter: None,
            limit: Some(10),
            min_score: Some(0.0),
            include_trace: false,
            tags_filter: None,
            source_filter: None,
            agent_id_filter: None,
            accessible_spaces: Vec::new(),
            conversation_context: None,
            project_path_filter: None,
        };

        let results = pipeline.search(&request, None).await.expect("search");
        assert!(!results.results.is_empty());

        let pinned_result = results
            .results
            .iter()
            .find(|r| r.memory.memory_type.is_pinned());
        let normal_result = results
            .results
            .iter()
            .find(|r| !r.memory.memory_type.is_pinned());

        if let (Some(p), Some(n)) = (pinned_result, normal_result) {
            assert!(
                p.score > n.score,
                "pinned ({}) should outscore normal ({})",
                p.score,
                n.score,
            );
        }
    }

    #[tokio::test]
    async fn test_min_score_filter() {
        let entries = vec![
            FusionEntry {
                memory: make_memory("t", "high", MemoryType::Insight),
                rrf_score: 0.8,
                bm25_score: 0.0,
                pre_rerank_score: 0.0,
            },
            FusionEntry {
                memory: make_memory("t", "low", MemoryType::Insight),
                rrf_score: 0.1,
                bm25_score: 0.0,
                pre_rerank_score: 0.0,
            },
            FusionEntry {
                memory: make_memory("t", "mid", MemoryType::Insight),
                rrf_score: 0.5,
                bm25_score: 0.0,
                pre_rerank_score: 0.0,
            },
        ];

        let (kept, stage) = RetrievalPipeline::stage_min_score_filter(entries, 0.3);
        assert_eq!(kept.len(), 2);
        assert_eq!(stage.dropped_ids.len(), 1);
        assert_eq!(stage.name, "min_score_filter");
        assert!(kept.iter().all(|e| e.rrf_score >= 0.3));
    }

    #[tokio::test]
    async fn test_fallback_vector_only() {
        let (store, _dir) = setup().await;

        let m1 = make_memory("t-001", "vector only content", MemoryType::Insight);
        let v1 = make_vector(0, 1.0);
        store.create(&m1, Some(&v1)).await.expect("create m1");

        let pipeline = RetrievalPipeline::new(store)
            .with_min_score(0.0)
            .with_hard_cutoff(0.0);

        let request = SearchRequest {
            query: "nonexistent fts query".to_string(),
            query_vector: Some(make_vector(0, 1.0)),
            tenant_id: "t-001".to_string(),
            scope_filter: None,
            limit: Some(10),
            min_score: Some(0.0),
            include_trace: true,
            tags_filter: None,
            source_filter: None,
            agent_id_filter: None,
            accessible_spaces: Vec::new(),
            conversation_context: None,
            project_path_filter: None,
        };

        let results = pipeline
            .search(&request, None)
            .await
            .expect("search should succeed even without FTS index");
        assert!(!results.results.is_empty());
    }

    #[tokio::test]
    async fn test_trace_output() {
        let (store, _dir) = setup().await;

        let m1 = make_memory("t-001", "trace test content", MemoryType::Insight);
        store
            .create(&m1, Some(&make_vector(0, 1.0)))
            .await
            .expect("create");

        let pipeline = RetrievalPipeline::new(store)
            .with_min_score(0.0)
            .with_hard_cutoff(0.0);

        let request = SearchRequest {
            query: "trace".to_string(),
            query_vector: Some(make_vector(0, 1.0)),
            tenant_id: "t-001".to_string(),
            scope_filter: None,
            limit: Some(10),
            min_score: Some(0.0),
            include_trace: true,
            tags_filter: None,
            source_filter: None,
            agent_id_filter: None,
            accessible_spaces: Vec::new(),
            conversation_context: None,
            project_path_filter: None,
        };

        let results = pipeline.search(&request, None).await.expect("search");
        let trace = &results.trace;

        assert_eq!(trace.stages.len(), 15);
        assert_eq!(trace.stages[0].name, "parallel_search");
        assert_eq!(trace.stages[1].name, "tag_boost");
        assert_eq!(trace.stages[2].name, "rrf_fusion");
        assert_eq!(trace.stages[3].name, "rrf_normalize");
        assert_eq!(trace.stages[4].name, "min_score_filter");
        assert_eq!(trace.stages[5].name, "topk_cap");
        assert_eq!(trace.stages[6].name, "expand_relations");
        assert_eq!(trace.stages[7].name, "cross_encoder_rerank");
        assert_eq!(trace.stages[8].name, "bm25_floor");
        assert_eq!(trace.stages[9].name, "decay_boost");
        assert_eq!(trace.stages[10].name, "importance_weight");
        assert_eq!(trace.stages[11].name, "length_normalization");
        assert_eq!(trace.stages[12].name, "hard_cutoff");
        assert_eq!(trace.stages[13].name, "mmr_diversity");
        assert_eq!(trace.stages[14].name, "llm_refine");

        let display = trace.to_string();
        assert!(display.contains("Retrieval Trace"));
        assert!(display.contains("parallel_search"));
        assert!(display.contains("mmr_diversity"));
    }

    #[tokio::test]
    async fn test_empty_results() {
        let (store, _dir) = setup().await;

        let pipeline = RetrievalPipeline::new(store);

        let request = SearchRequest {
            query: "nothing matches".to_string(),
            query_vector: Some(make_vector(0, 1.0)),
            tenant_id: "t-001".to_string(),
            scope_filter: None,
            limit: Some(10),
            min_score: None,
            include_trace: false,
            tags_filter: None,
            source_filter: None,
            agent_id_filter: None,
            accessible_spaces: Vec::new(),
            conversation_context: None,
            project_path_filter: None,
        };

        let results = pipeline
            .search(&request, None)
            .await
            .expect("search should not error on empty");
        assert!(results.results.is_empty());
        assert_eq!(results.trace.final_count, 0);
    }

    #[test]
    fn test_rerank_blends_scores() {
        let original_score = 0.5_f32;
        let rerank_score = 0.9_f32;
        let blended: f32 = rerank_score * 0.6 + original_score * 0.4;

        assert!((blended - 0.74_f32).abs() < 0.001_f32);

        let low_rerank = 0.1_f32;
        let blended_low = low_rerank * 0.6 + original_score * 0.4;
        assert!(blended > blended_low);
    }

    #[test]
    fn test_bm25_floor_preserved() {
        let mut entries = vec![
            FusionEntry {
                memory: make_memory("t", "JIRA-1234 ticket", MemoryType::Insight),
                rrf_score: 0.3,
                bm25_score: 0.9,
                pre_rerank_score: 0.8,
            },
            FusionEntry {
                memory: make_memory("t", "some other result", MemoryType::Insight),
                rrf_score: 0.3,
                bm25_score: 0.2,
                pre_rerank_score: 0.8,
            },
        ];

        entries[0].pre_rerank_score = 0.8;
        entries[1].pre_rerank_score = 0.8;

        let (result, _stage) = RetrievalPipeline::stage_bm25_floor(entries);

        let high_bm25 = &result[0];
        assert!(
            high_bm25.rrf_score >= 0.8 * 0.95 - f32::EPSILON,
            "high BM25 result should be floored to 95% of pre-rerank: got {}",
            high_bm25.rrf_score
        );

        let low_bm25 = &result[1];
        assert!(
            (low_bm25.rrf_score - 0.3).abs() < f32::EPSILON,
            "low BM25 result should be unchanged: got {}",
            low_bm25.rrf_score
        );
    }

    #[test]
    fn test_decay_boost_applied() {
        let engine = DecayEngine::new(DecayConfig {
            floor_peripheral: 0.0,
            ..DecayConfig::default()
        });

        let mut recent = make_memory("t", "recent memory", MemoryType::Insight);
        recent.access_count = 10;
        recent.importance = 0.8;
        recent.confidence = 0.8;

        let delta_old = chrono::TimeDelta::try_days(180).unwrap_or_default();
        let mut old = make_memory("t", "old memory", MemoryType::Insight);
        old.access_count = 1;
        old.importance = 0.3;
        old.confidence = 0.3;
        old.created_at = (chrono::Utc::now() - delta_old).to_rfc3339();
        old.last_accessed_at = None;

        let recent_boost = engine.apply_search_boost(1.0, &recent);
        let old_boost = engine.apply_search_boost(1.0, &old);

        assert!(
            recent_boost > old_boost,
            "recent memory ({recent_boost}) should get higher boost than old ({old_boost})"
        );
    }

    #[test]
    fn test_length_normalization() {
        let short_content = "short text";
        let long_content = "a ".repeat(1000);

        let short_entries = vec![make_entry(short_content, 1.0)];
        let (short_result, _) = RetrievalPipeline::stage_length_normalization(short_entries);
        let short_score = short_result[0].rrf_score;

        let long_entries = vec![make_entry(&long_content, 1.0)];
        let (long_result, _) = RetrievalPipeline::stage_length_normalization(long_entries);
        let long_score = long_result[0].rrf_score;

        assert!(
            short_score > long_score,
            "short ({short_score}) should score higher than long ({long_score})"
        );

        assert!(
            (short_score - 1.0).abs() < f32::EPSILON,
            "short content should not be penalized: got {short_score}"
        );
    }

    #[test]
    fn test_mmr_diversity() {
        let entries = vec![
            make_entry(
                "the quick brown fox jumps over the lazy dog and rests in the meadow today",
                0.9,
            ),
            make_entry(
                "the quick brown fox jumps over the lazy cat and rests in the meadow today",
                0.85,
            ),
            make_entry("completely different content about rust programming", 0.8),
        ];

        let (result, stage) = RetrievalPipeline::stage_mmr_diversity(entries, 10, 0.85, 0.5);
        assert_eq!(result.len(), 3);
        assert_eq!(stage.name, "mmr_diversity");

        let fox_dog = result.iter().find(|e| e.memory.content.contains("dog"));
        let fox_cat = result.iter().find(|e| e.memory.content.contains("cat"));
        let rust = result.iter().find(|e| e.memory.content.contains("rust"));

        if let (Some(dog), Some(cat), Some(r)) = (fox_dog, fox_cat, rust) {
            assert!(
                dog.rrf_score > cat.rrf_score,
                "near-duplicate should be penalized: dog={}, cat={}",
                dog.rrf_score,
                cat.rrf_score
            );
            assert!(
                r.rrf_score > cat.rrf_score,
                "unique result should rank above penalized duplicate: rust={}, cat={}",
                r.rrf_score,
                cat.rrf_score
            );
        }
    }

    #[test]
    fn test_hard_cutoff() {
        let entries = vec![
            make_entry("high", 0.8),
            make_entry("mid", 0.4),
            make_entry("low", 0.2),
            make_entry("very low", 0.1),
        ];

        let (kept, stage) = RetrievalPipeline::stage_hard_cutoff(entries, 0.35);
        assert_eq!(kept.len(), 2);
        assert_eq!(stage.dropped_ids.len(), 2);
        assert!(kept.iter().all(|e| e.rrf_score >= 0.35));
    }

    #[test]
    fn test_content_jaccard() {
        let a = "the quick brown fox";
        let b = "the quick brown fox";
        assert!((content_jaccard(a, b) - 1.0).abs() < f32::EPSILON);

        let c = "completely different words here";
        assert!(content_jaccard(a, c) < 0.2);

        assert!((content_jaccard("", "") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_rrf_normalize_multiple_results() {
        let entries = vec![
            make_entry("best", 0.033),
            make_entry("mid", 0.020),
            make_entry("worst", 0.010),
        ];

        let (result, stage) = RetrievalPipeline::stage_rrf_normalize(entries);
        assert_eq!(stage.name, "rrf_normalize");
        assert_eq!(result.len(), 3);

        let best = result.iter().find(|e| e.memory.content == "best").unwrap();
        let worst = result.iter().find(|e| e.memory.content == "worst").unwrap();
        let mid = result.iter().find(|e| e.memory.content == "mid").unwrap();

        assert!(
            (best.rrf_score - 1.0).abs() < 1e-6,
            "best should be 1.0, got {}",
            best.rrf_score
        );
        assert!(
            (worst.rrf_score - 0.0).abs() < 1e-6,
            "worst should be 0.0, got {}",
            worst.rrf_score
        );
        assert!(
            mid.rrf_score > 0.0 && mid.rrf_score < 1.0,
            "mid should be between 0 and 1, got {}",
            mid.rrf_score
        );
    }

    #[test]
    fn test_rrf_normalize_single_result() {
        let entries = vec![make_entry("only", 0.008)];

        let (result, _) = RetrievalPipeline::stage_rrf_normalize(entries);
        assert_eq!(result.len(), 1);
        let score = result[0].rrf_score;
        assert!(
            (score - 0.96).abs() < 1e-4,
            "0.008 * 120 = 0.96, got {score}"
        );
    }

    #[test]
    fn test_rrf_normalize_single_result_clamped() {
        let entries = vec![make_entry("high", 0.05)];

        let (result, _) = RetrievalPipeline::stage_rrf_normalize(entries);
        assert!(
            (result[0].rrf_score - 1.0).abs() < 1e-6,
            "should clamp to 1.0, got {}",
            result[0].rrf_score
        );
    }

    #[test]
    fn test_rrf_normalize_equal_scores() {
        let entries = vec![make_entry("a", 0.016), make_entry("b", 0.016)];

        let (result, _) = RetrievalPipeline::stage_rrf_normalize(entries);
        assert!((result[0].rrf_score - 1.0).abs() < 1e-6);
        assert!((result[1].rrf_score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_rrf_normalize_empty() {
        let entries: Vec<FusionEntry> = vec![];
        let (result, _) = RetrievalPipeline::stage_rrf_normalize(entries);
        assert!(result.is_empty());
    }
}
