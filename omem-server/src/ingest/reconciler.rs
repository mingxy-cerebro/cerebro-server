use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::domain::category::Category;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::relation::{MemoryRelation, RelationType};
use crate::domain::types::MemoryType;
use crate::embed::EmbedService;
use crate::ingest::preference_slots;
use crate::ingest::prompts;
use crate::ingest::types::{BatchDedupResult, ExtractedFact, ReconcileResult};
use crate::llm::{complete_json, LlmService};
use crate::store::LanceStore;

const DEFAULT_MAX_EXISTING: usize = 150;
const DEFAULT_MAX_PER_FACT: usize = 20;
const DEFAULT_MIN_SIMILARITY: f32 = 0.3;

pub struct Reconciler {
    llm: Arc<dyn LlmService>,
    store: Arc<LanceStore>,
    embed: Arc<dyn EmbedService>,
    max_existing: usize,
    max_per_fact: usize,
    min_similarity: f32,
}

impl Reconciler {
    pub fn new(
        llm: Arc<dyn LlmService>,
        store: Arc<LanceStore>,
        embed: Arc<dyn EmbedService>,
    ) -> Self {
        Self {
            llm,
            store,
            embed,
            max_existing: DEFAULT_MAX_EXISTING,
            max_per_fact: DEFAULT_MAX_PER_FACT,
            min_similarity: DEFAULT_MIN_SIMILARITY,
        }
    }

    pub async fn reconcile(
        &self,
        facts: &[ExtractedFact],
        tenant_id: &str,
        agent_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<Vec<Memory>, OmemError> {
        if facts.is_empty() {
            return Ok(Vec::new());
        }

        let (existing, all_searches_failed) = self.gather_existing(facts, session_id.clone()).await;

        if existing.is_empty() && all_searches_failed {
            return Err(OmemError::Internal(
                "all searches failed during reconciliation — refusing to create duplicates"
                    .to_string(),
            ));
        }

        // 2c: batch self-dedup always runs (regardless of whether existing is empty)
        let facts: Vec<ExtractedFact> = if facts.len() > 1 {
            self.batch_self_dedup(facts).await?
        } else {
            facts.to_vec()
        };

        let mut created_memories = Vec::new();

        // 2a: exact match dedup (hard hash + substring) against existing memories
        let (exact_skipped, exact_upgraded, facts) = self.exact_match_dedup(&facts, &existing).await?;
        if exact_skipped > 0 {
            info!(
                count = exact_skipped,
                "facts skipped by exact match dedup (existing has higher or equal importance)"
            );
        }
        if !exact_upgraded.is_empty() {
            info!(
                count = exact_upgraded.len(),
                "existing memories upgraded by exact match dedup (incoming has higher importance)"
            );
            created_memories.extend(exact_upgraded);
        }

        if facts.is_empty() {
            return Ok(created_memories);
        }

        if existing.is_empty() {
            return self.create_all_facts(&facts, tenant_id, agent_id.clone(), session_id.clone()).await;
        }

        let (fast_merged, facts) = if let Some(ref sid) = session_id {
            self.fast_session_merge(&facts, &existing, sid, tenant_id).await?
        } else {
            (Vec::new(), facts)
        };
        if !fast_merged.is_empty() {
            info!(
                count = fast_merged.len(),
                "facts merged by fast session merge path"
            );
            created_memories.extend(fast_merged);
        }
        if facts.is_empty() {
            return Ok(created_memories);
        }

        let mut remaining_facts: Vec<(usize, &ExtractedFact)> = Vec::new();

        for (idx, fact) in facts.iter().enumerate() {
            if self.preference_slot_guard(fact, &existing, agent_id.clone(), session_id.clone()).await? {
                let mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
                created_memories.push(mem);
            } else {
                remaining_facts.push((idx, fact));
            }
        }

        if remaining_facts.is_empty() {
            return Ok(created_memories);
        }

        let remaining_extracted: Vec<ExtractedFact> =
            remaining_facts.iter().map(|(_, f)| (*f).clone()).collect();

        // 2d: fuzzy dedup pairs among remaining facts
        let fuzzy_pairs = compute_fuzzy_pairs(&remaining_extracted);

        let (id_map, int_to_uuid) = build_id_maps(&existing);

        let (system, user) =
            prompts::build_reconcile_prompt(&remaining_extracted, &existing, &id_map, &fuzzy_pairs);
        let result: ReconcileResult = complete_json(self.llm.as_ref(), &system, &user).await?;

        for decision in &result.decisions {
            let action = decision.action.to_uppercase();
            let (_, fact) = remaining_facts.get(decision.fact_index).ok_or_else(|| {
                OmemError::Llm(format!("invalid fact_index: {}", decision.fact_index))
            })?;

            match action.as_str() {
                "CREATE" => {
                    let mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
                    created_memories.push(mem);
                }
                "MERGE" => {
                    let match_idx = decision.match_index.ok_or_else(|| {
                        OmemError::Llm("MERGE decision missing match_index".to_string())
                    })?;
                    let real_id = int_to_uuid.get(&match_idx).ok_or_else(|| {
                        OmemError::Llm(format!("invalid match_index: {match_idx}"))
                    })?;

                    let target = self
                        .store
                        .get_by_id(real_id)
                        .await?
                        .ok_or_else(|| OmemError::NotFound(format!("memory {real_id}")))?;

                    if target.memory_type.is_pinned() {
                warn!(
                    memory_id = %real_id,
                    "MERGE attempted on pinned memory — downgrading to CREATE"
                );
                let mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
                created_memories.push(mem);
                        continue;
                    }

                    let merged_content = decision
                        .merged_content
                        .as_deref()
                        .unwrap_or(&fact.l2_content);

                    let mut updated = target;
                    updated.content = merged_content.to_string();
                    updated.l0_abstract = merged_content.to_string();
                    updated.updated_at = chrono::Utc::now().to_rfc3339();

                    let embeddings = self.embed.embed(&[merged_content.to_string()]).await?;
                    let vector = embeddings.first().map(|v| v.as_slice());

                    self.store.update(&updated, vector).await?;
                    created_memories.push(updated);
                }
                "SKIP" => {}
                "SUPERSEDE" => {
                    self.handle_supersede(
                        fact,
                        &decision.match_index,
                        &int_to_uuid,
                        tenant_id,
                        &mut created_memories,
                        agent_id.clone(),
                        session_id.clone(),
                    )
                    .await?;
                }
                "SUPPORT" => {
                    self.handle_support(
                        fact,
                        &decision.match_index,
                        &decision.context_label,
                        &int_to_uuid,
                        &mut created_memories,
                    )
                    .await?;
                }
                "CONTEXTUALIZE" => {
                    self.handle_contextualize(
                        fact,
                        &decision.match_index,
                        &decision.context_label,
                        &int_to_uuid,
                        tenant_id,
                        &mut created_memories,
                        agent_id.clone(),
                        session_id.clone(),
                    )
                    .await?;
                }
                "CONTRADICT" => {
                    self.handle_contradict(
                        fact,
                        &decision.match_index,
                        &int_to_uuid,
                        tenant_id,
                        &mut created_memories,
                        agent_id.clone(),
                        session_id.clone(),
                    )
                    .await?;
                }
                other => {
                    warn!(action = %other, "unknown reconciliation action — treating as CREATE");
                    let mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
                    created_memories.push(mem);
                }
            }
        }

        Ok(created_memories)
    }

    async fn fast_session_merge(
        &self,
        facts: &[ExtractedFact],
        existing: &[Memory],
        session_id: &str,
        _tenant_id: &str,
    ) -> Result<(Vec<Memory>, Vec<ExtractedFact>), OmemError> {
        let session_memories: Vec<&Memory> = existing
            .iter()
            .filter(|m| m.session_id.as_deref() == Some(session_id))
            .collect();

        if session_memories.is_empty() {
            return Ok((Vec::new(), facts.to_vec()));
        }

        let mut merged = Vec::new();
        let mut remaining = Vec::new();

        for fact in facts {
            let mut best_match: Option<(f32, &Memory)> = None;
            for mem in &session_memories {
                let sim = jaccard_similarity(&fact.l0_abstract, &mem.l0_abstract);
                if sim > 0.5 {
                    match best_match {
                        None => best_match = Some((sim, mem)),
                        Some((best_sim, _)) if sim > best_sim => best_match = Some((sim, mem)),
                        _ => {}
                    }
                }
            }

            if let Some((_sim, existing_mem)) = best_match {
                if existing_mem.memory_type.is_pinned() {
                    remaining.push(fact.clone());
                    continue;
                }

                let merged_content = fact.source_text.as_deref().unwrap_or(&fact.l0_abstract);
                let mut updated = (*existing_mem).clone();
                updated.content = merged_content.to_string();
                updated.l0_abstract = fact.l0_abstract.clone();
                updated.l1_overview = fact.l1_overview.clone();
                updated.l2_content = fact.l2_content.clone();
                updated.tags = fact.tags.clone();
                updated.confidence = fact.quality_score.clamp(0.1, 1.0);
                let category: Category = fact.category.parse().unwrap_or(Category::Profile);
                updated.importance = category_importance(&category, fact.quality_score);
                updated.updated_at = chrono::Utc::now().to_rfc3339();

                let embeddings = self.embed.embed(&[updated.l0_abstract.clone()]).await?;
                let vector = embeddings.first().map(|v| v.as_slice());

                self.store.update(&updated, vector).await?;
                merged.push(updated);
            } else {
                remaining.push(fact.clone());
            }
        }

        Ok((merged, remaining))
    }

    async fn preference_slot_guard(
        &self,
        fact: &ExtractedFact,
        existing: &[Memory],
        _agent_id: Option<String>,
        _session_id: Option<String>,
    ) -> Result<bool, OmemError> {
        let category: Category = fact.category.parse().unwrap_or(Category::Profile);
        if category != Category::Preferences {
            return Ok(false);
        }

        let candidate_slot = match preference_slots::infer_preference_slot(&fact.l0_abstract) {
            Some(s) => s,
            None => return Ok(false),
        };

        for mem in existing {
            if mem.category != Category::Preferences {
                continue;
            }
            if let Some(existing_slot) = preference_slots::infer_preference_slot(&mem.l0_abstract) {
                if preference_slots::is_same_brand_different_item(&candidate_slot, &existing_slot) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    async fn handle_supersede(
        &self,
        fact: &ExtractedFact,
        match_index: &Option<usize>,
        int_to_uuid: &HashMap<usize, String>,
        tenant_id: &str,
        created_memories: &mut Vec<Memory>,
        agent_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<(), OmemError> {
        let match_idx = match_index
            .ok_or_else(|| OmemError::Llm("SUPERSEDE decision missing match_index".to_string()))?;
        let real_id = int_to_uuid
            .get(&match_idx)
            .ok_or_else(|| OmemError::Llm(format!("invalid match_index: {match_idx}")))?;

        let old = self
            .store
            .get_by_id(real_id)
            .await?
            .ok_or_else(|| OmemError::NotFound(format!("memory {real_id}")))?;

        if old.memory_type.is_pinned() {
            warn!(
                memory_id = %real_id,
                "SUPERSEDE attempted on pinned memory — downgrading to CREATE"
            );
            let mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
            created_memories.push(mem);
            return Ok(());
        }

        let new_mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;

        let mut archived = old;
        archived.invalidated_at = Some(chrono::Utc::now().to_rfc3339());
        archived.superseded_by = Some(new_mem.id.clone());
        archived.updated_at = chrono::Utc::now().to_rfc3339();
        self.store.update(&archived, None).await?;

        created_memories.push(new_mem);
        Ok(())
    }

    async fn handle_support(
        &self,
        _fact: &ExtractedFact,
        match_index: &Option<usize>,
        context_label: &Option<String>,
        int_to_uuid: &HashMap<usize, String>,
        created_memories: &mut Vec<Memory>,
    ) -> Result<(), OmemError> {
        let match_idx = match_index
            .ok_or_else(|| OmemError::Llm("SUPPORT decision missing match_index".to_string()))?;
        let real_id = int_to_uuid
            .get(&match_idx)
            .ok_or_else(|| OmemError::Llm(format!("invalid match_index: {match_idx}")))?;

        let mut target = self
            .store
            .get_by_id(real_id)
            .await?
            .ok_or_else(|| OmemError::NotFound(format!("memory {real_id}")))?;

        target.confidence = (target.confidence + 0.1).min(1.0);
        target.relations.push(MemoryRelation {
            relation_type: RelationType::Supports,
            target_id: real_id.clone(),
            context_label: context_label.clone(),
        });
        target.updated_at = chrono::Utc::now().to_rfc3339();

        self.store.update(&target, None).await?;
        created_memories.push(target);
        Ok(())
    }

    async fn handle_contextualize(
        &self,
        fact: &ExtractedFact,
        match_index: &Option<usize>,
        context_label: &Option<String>,
        int_to_uuid: &HashMap<usize, String>,
        tenant_id: &str,
        created_memories: &mut Vec<Memory>,
        agent_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<(), OmemError> {
        let match_idx = match_index.ok_or_else(|| {
            OmemError::Llm("CONTEXTUALIZE decision missing match_index".to_string())
        })?;
        let real_id = int_to_uuid
            .get(&match_idx)
            .ok_or_else(|| OmemError::Llm(format!("invalid match_index: {match_idx}")))?;

        let mut new_mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
        new_mem.relations.push(MemoryRelation {
            relation_type: RelationType::Contextualizes,
            target_id: real_id.clone(),
            context_label: context_label.clone(),
        });

        let embeddings = self.embed.embed(std::slice::from_ref(&fact.l0_abstract)).await?;
        let vector = embeddings.first().map(|v| v.as_slice());
        self.store.update(&new_mem, vector).await?;

        created_memories.push(new_mem);
        Ok(())
    }

    async fn handle_contradict(
        &self,
        fact: &ExtractedFact,
        match_index: &Option<usize>,
        int_to_uuid: &HashMap<usize, String>,
        tenant_id: &str,
        created_memories: &mut Vec<Memory>,
        agent_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<(), OmemError> {
        let match_idx = match_index
            .ok_or_else(|| OmemError::Llm("CONTRADICT decision missing match_index".to_string()))?;
        let real_id = int_to_uuid
            .get(&match_idx)
            .ok_or_else(|| OmemError::Llm(format!("invalid match_index: {match_idx}")))?;

        let old = self
            .store
            .get_by_id(real_id)
            .await?
            .ok_or_else(|| OmemError::NotFound(format!("memory {real_id}")))?;

        let category: Category = fact.category.parse().unwrap_or(Category::Profile);
        if category.is_temporal_versioned() {
            return self
                .handle_supersede(fact, match_index, int_to_uuid, tenant_id, created_memories, agent_id.clone(), session_id.clone())
                .await;
        }

        let mut new_mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
        new_mem.relations.push(MemoryRelation {
            relation_type: RelationType::Contradicts,
            target_id: real_id.clone(),
            context_label: None,
        });

        let embeddings = self.embed.embed(std::slice::from_ref(&fact.l0_abstract)).await?;
        let vector = embeddings.first().map(|v| v.as_slice());
        self.store.update(&new_mem, vector).await?;

        let mut old_updated = old;
        old_updated.relations.push(MemoryRelation {
            relation_type: RelationType::Contradicts,
            target_id: new_mem.id.clone(),
            context_label: None,
        });
        old_updated.updated_at = chrono::Utc::now().to_rfc3339();
        self.store.update(&old_updated, None).await?;

        created_memories.push(new_mem);
        Ok(())
    }

    async fn batch_self_dedup(
        &self,
        facts: &[ExtractedFact],
    ) -> Result<Vec<ExtractedFact>, OmemError> {
        let (system, user) = prompts::build_batch_dedup_prompt(facts);

        let result: BatchDedupResult = complete_json(self.llm.as_ref(), &system, &user).await?;

        let deduped: Vec<ExtractedFact> = result
            .keep_indices
            .iter()
            .filter_map(|&idx| facts.get(idx).cloned())
            .collect();

        if deduped.is_empty() {
            // Safety: if LLM returns garbage, keep all facts
            Ok(facts.to_vec())
        } else {
            info!(
                original = facts.len(),
                deduped = deduped.len(),
                removed = facts.len() - deduped.len(),
                "batch self-dedup completed"
            );
            Ok(deduped)
        }
    }

    async fn gather_existing(
        &self,
        facts: &[ExtractedFact],
        session_id: Option<String>,
    ) -> (Vec<Memory>, bool) {
        let mut seen_ids: HashMap<String, Memory> = HashMap::new();
        let mut any_search_succeeded = false;
        let mut total_count = 0;

        if let Some(ref sid) = session_id {
            match self.store.find_memories_by_session_id(sid, self.max_existing).await {
                Ok(results) => {
                    any_search_succeeded = true;
                    for mem in results {
                        if total_count >= self.max_existing {
                            break;
                        }
                        if !seen_ids.contains_key(&mem.id) {
                            seen_ids.insert(mem.id.clone(), mem);
                            total_count += 1;
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "session_id search failed during gather");
                }
            }
        }

        for fact in facts {
            if total_count >= self.max_existing {
                break;
            }

            let search_text = fact.source_text.as_deref().unwrap_or(&fact.l0_abstract);

            let embed_result = self
                .embed
                .embed(std::slice::from_ref(&search_text.to_string()))
                .await;

            if let Ok(vectors) = embed_result {
                if let Some(query_vec) = vectors.first() {
                    match self
                        .store
                        .vector_search(
                            query_vec,
                            self.max_per_fact,
                            self.min_similarity,
                            None,
                            None,
                            None,
                        )
                        .await
                    {
                        Ok(results) => {
                            any_search_succeeded = true;
                            for (mem, _score) in results {
                                if total_count >= self.max_existing {
                                    break;
                                }
                                if !seen_ids.contains_key(&mem.id) {
                                    seen_ids.insert(mem.id.clone(), mem);
                                    total_count += 1;
                                }
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "vector search failed during gather");
                        }
                    }
                }
            } else {
                warn!("embedding failed during gather");
            }

            let fts_query = fact
                .source_text
                .as_deref()
                .map(|s| s.chars().take(200).collect::<String>())
                .unwrap_or_else(|| fact.l0_abstract.clone());

            match self
                .store
                .fts_search(&fts_query, self.max_per_fact, None, None, None)
                .await
            {
                Ok(results) => {
                    any_search_succeeded = true;
                    for (mem, _score) in results {
                        if total_count >= self.max_existing {
                            break;
                        }
                        if !seen_ids.contains_key(&mem.id) {
                            seen_ids.insert(mem.id.clone(), mem);
                            total_count += 1;
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "FTS search failed during gather");
                }
            }
        }

        let all_failed = !any_search_succeeded;
        (seen_ids.into_values().collect(), all_failed)
    }

    async fn create_all_facts(
        &self,
        facts: &[ExtractedFact],
        tenant_id: &str,
        agent_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<Vec<Memory>, OmemError> {
        let mut memories = Vec::with_capacity(facts.len());
        for fact in facts {
            let mem = self.create_fact_memory(fact, tenant_id, agent_id.clone(), session_id.clone()).await?;
            memories.push(mem);
        }
        Ok(memories)
    }

    async fn create_fact_memory(
        &self,
        fact: &ExtractedFact,
        tenant_id: &str,
        agent_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<Memory, OmemError> {
        let category: Category = fact.category.parse().unwrap_or(Category::Profile);

        let source = fact.source_text.as_deref().unwrap_or(&fact.l0_abstract);

        let mut mem = Memory::new(source, category, MemoryType::Insight, tenant_id);
        mem.l0_abstract = fact.l0_abstract.clone();
        mem.l1_overview = fact.l1_overview.clone();
        mem.l2_content = fact.l2_content.clone();
        mem.tags = fact.tags.clone();
        mem.confidence = fact.quality_score.clamp(0.1, 1.0);
        mem.importance = category_importance(&mem.category, fact.quality_score);
        mem.agent_id = agent_id;
        mem.session_id = session_id;
        mem.source = Some("ingest".to_string());
        mem.visibility = fact.visibility.clone();
        mem.owner_agent_id = fact.owner_agent_id.clone();

        // Use l0_abstract for embedding (semantic summary matches short queries better than raw conversation)
        let embed_source = &fact.l0_abstract;
        let embeddings = self
            .embed
            .embed(std::slice::from_ref(embed_source))
            .await?;
        let vector = embeddings.first().map(|v| v.as_slice());

        self.store.create(&mem, vector).await?;
        Ok(mem)
    }

    async fn exact_match_dedup(
        &self,
        facts: &[ExtractedFact],
        existing: &[Memory],
    ) -> Result<(usize, Vec<Memory>, Vec<ExtractedFact>), OmemError> {
        if existing.is_empty() || facts.is_empty() {
            return Ok((0, Vec::new(), facts.to_vec()));
        }

        let mut existing_by_hash: HashMap<String, &Memory> = HashMap::new();
        for mem in existing {
            let normalized = normalize_for_dedup(&mem.content);
            let hash = content_hash(&normalized);
            existing_by_hash.insert(hash, mem);
        }

        let mut skipped = 0;
        let mut upgraded = Vec::new();
        let mut remaining = Vec::with_capacity(facts.len());

        for fact in facts {
            let fact_content = fact.source_text.as_deref().unwrap_or(&fact.l0_abstract);
            let normalized_fact = normalize_for_dedup(fact_content);
            let fact_hash = content_hash(&normalized_fact);

            let mut is_duplicate = false;
            let category: Category = fact.category.parse().unwrap_or(Category::Profile);
            let fact_importance = category_importance(&category, fact.quality_score);

            // Hard hash check
            if let Some(existing_mem) = existing_by_hash.get(&fact_hash) {
                if existing_mem.importance >= fact_importance {
                    debug!(
                        fact_hash = %fact_hash,
                        existing_id = %existing_mem.id,
                        existing_importance = existing_mem.importance,
                        fact_importance = fact_importance,
                        "hard hash match: existing has higher or equal importance, skipping"
                    );
                    skipped += 1;
                } else {
                    debug!(
                        fact_hash = %fact_hash,
                        existing_id = %existing_mem.id,
                        existing_importance = existing_mem.importance,
                        fact_importance = fact_importance,
                        "hard hash match: fact has higher importance, upgrading existing"
                    );
                    let mut updated = (*existing_mem).clone();
                    updated.content = fact_content.to_string();
                    updated.l0_abstract = fact.l0_abstract.clone();
                    updated.l1_overview = fact.l1_overview.clone();
                    updated.l2_content = fact.l2_content.clone();
                    updated.tags = fact.tags.clone();
                    updated.confidence = fact.quality_score.clamp(0.1, 1.0);
                    updated.importance = fact_importance;
                    updated.updated_at = chrono::Utc::now().to_rfc3339();

                    let embeddings = self.embed.embed(&[fact_content.to_string()]).await?;
                    let vector = embeddings.first().map(|v| v.as_slice());
                    self.store.update(&updated, vector).await?;
                    upgraded.push(updated);
                }
                continue;
            }

            // Substring check on l0_abstract
            if !is_duplicate {
                let fact_l0_lower = fact.l0_abstract.to_lowercase();
                for existing_mem in existing {
                    let existing_l0_lower = existing_mem.l0_abstract.to_lowercase();
                    if fact_l0_lower.contains(&existing_l0_lower) || existing_l0_lower.contains(&fact_l0_lower) {
                        is_duplicate = true;
                        if existing_mem.importance >= fact_importance {
                            debug!(
                                fact_l0 = %fact.l0_abstract,
                                existing_id = %existing_mem.id,
                                existing_importance = existing_mem.importance,
                                fact_importance = fact_importance,
                                "substring match: existing has higher or equal importance, skipping"
                            );
                            skipped += 1;
                        } else {
                            debug!(
                                fact_l0 = %fact.l0_abstract,
                                existing_id = %existing_mem.id,
                                existing_importance = existing_mem.importance,
                                fact_importance = fact_importance,
                                "substring match: fact has higher importance, upgrading existing"
                            );
                            let mut updated = (*existing_mem).clone();
                            updated.content = fact_content.to_string();
                            updated.l0_abstract = fact.l0_abstract.clone();
                            updated.l1_overview = fact.l1_overview.clone();
                            updated.l2_content = fact.l2_content.clone();
                            updated.tags = fact.tags.clone();
                            updated.confidence = fact.quality_score.clamp(0.1, 1.0);
                            updated.importance = fact_importance;
                            updated.updated_at = chrono::Utc::now().to_rfc3339();

                            let embeddings = self.embed.embed(&[fact_content.to_string()]).await?;
                            let vector = embeddings.first().map(|v| v.as_slice());
                            self.store.update(&updated, vector).await?;
                            upgraded.push(updated);
                        }
                        break;
                    }
                }
            }

            // Jaccard similarity check (catches semantic near-duplicates like "ocosay重构完成" vs "ocosay重构九项任务完成")
            if !is_duplicate {
                let mut best_match: Option<(f32, &Memory)> = None;
                for existing_mem in existing {
                    let sim = jaccard_similarity(&fact.l0_abstract, &existing_mem.l0_abstract);
                    if sim > 0.6 {
                        match best_match {
                            None => best_match = Some((sim, existing_mem)),
                            Some((best_sim, _)) if sim > best_sim => best_match = Some((sim, existing_mem)),
                            _ => {}
                        }
                    }
                }
                if let Some((sim, existing_mem)) = best_match {
                    is_duplicate = true;
                    if existing_mem.importance >= fact_importance {
                        debug!(
                            fact_l0 = %fact.l0_abstract,
                            existing_id = %existing_mem.id,
                            jaccard = sim,
                            "jaccard match: existing has higher or equal importance, skipping"
                        );
                        skipped += 1;
                    } else {
                        debug!(
                            fact_l0 = %fact.l0_abstract,
                            existing_id = %existing_mem.id,
                            jaccard = sim,
                            "jaccard match: fact has higher importance, upgrading existing"
                        );
                        let mut updated = (*existing_mem).clone();
                        updated.content = fact_content.to_string();
                        updated.l0_abstract = fact.l0_abstract.clone();
                        updated.l1_overview = fact.l1_overview.clone();
                        updated.l2_content = fact.l2_content.clone();
                        updated.tags = fact.tags.clone();
                        updated.confidence = fact.quality_score.clamp(0.1, 1.0);
                        updated.importance = fact_importance;
                        updated.updated_at = chrono::Utc::now().to_rfc3339();

                        let embeddings = self.embed.embed(&[fact_content.to_string()]).await?;
                        let vector = embeddings.first().map(|v| v.as_slice());
                        self.store.update(&updated, vector).await?;
                        upgraded.push(updated);
                    }
                }
            }

            if !is_duplicate {
                remaining.push(fact.clone());
            }
        }

        Ok((skipped, upgraded, remaining))
    }
}

fn normalize_for_dedup(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut prev_was_space = true;

    for ch in content.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
            prev_was_space = false;
        } else if !prev_was_space {
            result.push(' ');
            prev_was_space = true;
        }
    }

    if result.ends_with(' ') {
        result.pop();
    }

    result
}

fn content_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    content.trim().to_lowercase().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn jaccard_similarity(a: &str, b: &str) -> f32 {
    let norm_a = normalize_for_dedup(a);
    let norm_b = normalize_for_dedup(b);

    let chars_a: Vec<char> = norm_a.chars().collect();
    let chars_b: Vec<char> = norm_b.chars().collect();

    let grams_a: HashSet<String> = chars_a.windows(3).map(|w| w.iter().collect()).collect();
    let grams_b: HashSet<String> = chars_b.windows(3).map(|w| w.iter().collect()).collect();

    if grams_a.is_empty() && grams_b.is_empty() {
        return 1.0;
    }
    if grams_a.is_empty() || grams_b.is_empty() {
        return 0.0;
    }

    let intersection = grams_a.intersection(&grams_b).count();
    let union = grams_a.union(&grams_b).count();

    intersection as f32 / union as f32
}

fn compute_fuzzy_pairs(facts: &[ExtractedFact]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for i in 0..facts.len() {
        for j in (i + 1)..facts.len() {
            let sim = jaccard_similarity(&facts[i].l0_abstract, &facts[j].l0_abstract);
            if sim > 0.85 {
                pairs.push((i, j));
            }
        }
    }
    pairs
}

fn category_importance(category: &Category, quality_score: f32) -> f32 {
    let base = match category {
        Category::Profile => 0.8,
        Category::Preferences => 0.7,
        Category::Entities => 0.6,
        Category::Patterns => 0.6,
        Category::Cases => 0.5,
        Category::Events => 0.4,
    };
    let blended = base * 0.6 + quality_score * 0.4;
    blended.clamp(0.1, 1.0)
}

fn build_id_maps(existing: &[Memory]) -> (Vec<(usize, &str)>, HashMap<usize, String>) {
    let id_map: Vec<(usize, &str)> = existing
        .iter()
        .enumerate()
        .map(|(i, m)| (i, m.id.as_str()))
        .collect();

    let int_to_uuid: HashMap<usize, String> = id_map
        .iter()
        .map(|(i, uuid)| (*i, uuid.to_string()))
        .collect();

    (id_map, int_to_uuid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::types::ExtractedFact;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct MockLlm {
        response: Mutex<String>,
    }

    impl MockLlm {
        fn new(json_response: &str) -> Self {
            Self {
                response: Mutex::new(json_response.to_string()),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmService for MockLlm {
        async fn complete_text(&self, _system: &str, _user: &str) -> Result<String, OmemError> {
            Ok(self.response.lock().expect("lock").clone())
        }
    }

    struct MockEmbed;

    #[async_trait::async_trait]
    impl EmbedService for MockEmbed {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, OmemError> {
            Ok(texts.iter().map(|_| vec![0.0; 1024]).collect())
        }
        fn dimensions(&self) -> usize {
            1024
        }
    }

    struct CapturingLlm {
        response: Mutex<String>,
        captured_user: Mutex<Option<String>>,
    }

    impl CapturingLlm {
        fn new(json_response: &str) -> Self {
            Self {
                response: Mutex::new(json_response.to_string()),
                captured_user: Mutex::new(None),
            }
        }

        fn captured_user(&self) -> Option<String> {
            self.captured_user.lock().expect("lock").clone()
        }
    }

    #[async_trait::async_trait]
    impl LlmService for CapturingLlm {
        async fn complete_text(&self, _system: &str, user: &str) -> Result<String, OmemError> {
            *self.captured_user.lock().expect("lock") = Some(user.to_string());
            Ok(self.response.lock().expect("lock").clone())
        }
    }

    async fn setup() -> (Arc<LanceStore>, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let store = LanceStore::new(dir.path().to_str().expect("path"))
            .await
            .expect("store");
        store.init_table().await.expect("init");
        (Arc::new(store), dir)
    }

    fn make_fact(abstract_text: &str, category: &str) -> ExtractedFact {
        ExtractedFact {
            l0_abstract: abstract_text.to_string(),
            l1_overview: format!("Overview: {abstract_text}"),
            l2_content: format!("Detail: {abstract_text}"),
            category: category.to_string(),
            tags: vec![],
            source_text: None,
            quality_score: 0.0,
            visibility: "global".to_string(),
            owner_agent_id: String::new(),
            llm_confidence: 0,
        }
    }

    #[tokio::test]
    async fn test_reconcile_empty_store() {
        let (store, _dir) = setup().await;
        let llm = Arc::new(MockLlm::new(r#"{"keep_indices": [0, 1]}"#));
        let embed = Arc::new(MockEmbed);

        let reconciler = Reconciler::new(llm, store.clone(), embed);

        let facts = vec![
            make_fact("User prefers Rust", "preferences"),
            make_fact("User works at Stripe", "profile"),
        ];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].l0_abstract, "User prefers Rust");
        assert_eq!(result[0].tenant_id, "t-001");
        assert_eq!(result[0].memory_type, MemoryType::Insight);
        assert_eq!(result[1].l0_abstract, "User works at Stripe");
    }

    #[tokio::test]
    async fn test_reconcile_skip_duplicate() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let existing = Memory::new(
            "User prefers Rust",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let skip_response = r#"{"decisions":[{"action":"SKIP","fact_index":0,"match_index":0,"reason":"duplicate"}]}"#;
        let llm = Arc::new(MockLlm::new(skip_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("User prefers Rust", "preferences")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_reconcile_merge() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User prefers Rust",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User prefers Rust".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let merge_response = r#"{"decisions":[{"action":"MERGE","fact_index":0,"match_index":0,"merged_content":"User prefers Rust for its safety and performance","reason":"adds detail"}]}"#;
        let llm = Arc::new(MockLlm::new(merge_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact(
            "User likes Rust for safety and performance",
            "preferences",
        )];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].content,
            "User prefers Rust for its safety and performance"
        );

        let updated = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert_eq!(
            updated.content,
            "User prefers Rust for its safety and performance"
        );
    }

    #[tokio::test]
    async fn test_reconcile_supersede() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User works at Google",
            Category::Profile,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User works at Google".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let supersede_response = r#"{"decisions":[{"action":"SUPERSEDE","fact_index":0,"match_index":0,"reason":"user changed jobs"}]}"#;
        let llm = Arc::new(MockLlm::new(supersede_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("User now works at Stripe", "profile")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].l0_abstract, "User now works at Stripe");

        let old = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert!(old.invalidated_at.is_some());
        assert_eq!(old.superseded_by.as_deref(), Some(result[0].id.as_str()));
    }

    #[tokio::test]
    async fn test_pinned_protection() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut pinned = Memory::new(
            "Important: always use HTTPS",
            Category::Preferences,
            MemoryType::Pinned,
            "t-001",
        );
        pinned.l0_abstract = "Important: always use HTTPS".to_string();
        store
            .create(&pinned, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let merge_response = r#"{"decisions":[{"action":"MERGE","fact_index":0,"match_index":0,"merged_content":"merged text","reason":"refine"}]}"#;
        let llm = Arc::new(MockLlm::new(merge_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("Use HTTPS everywhere", "preferences")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");

        assert_eq!(result.len(), 1);
        assert_ne!(result[0].id, pinned.id);
        assert_eq!(result[0].memory_type, MemoryType::Insight);

        let original = store
            .get_by_id(&pinned.id)
            .await
            .expect("get")
            .expect("found");
        assert_eq!(original.content, "Important: always use HTTPS");
        assert_eq!(original.memory_type, MemoryType::Pinned);
    }

    #[tokio::test]
    async fn test_uuid_to_int_mapping() {
        let (store, _dir) = setup().await;

        let mut m1 = Memory::new("Fact A original", Category::Profile, MemoryType::Insight, "t-001");
        m1.l0_abstract = "Fact A original".to_string();
        let mut m2 = Memory::new(
            "Fact B",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        m2.l0_abstract = "Fact B".to_string();

        store
            .create(&m1, Some(&vec![0.0; 1024]))
            .await
            .expect("create");
        store
            .create(&m2, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let skip_response =
            r#"{"decisions":[{"action":"SKIP","fact_index":0,"match_index":0,"reason":"dup"}]}"#;
        let llm = Arc::new(CapturingLlm::new(skip_response));
        let embed = Arc::new(MockEmbed);

        let reconciler = Reconciler::new(llm.clone(), store.clone(), embed);
        let facts = vec![make_fact("Fact A", "profile")];

        let _ = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");

        let captured = llm.captured_user().expect("captured");
        assert!(
            !captured.contains(&m1.id),
            "prompt should not contain raw UUID"
        );
        assert!(
            !captured.contains(&m2.id),
            "prompt should not contain raw UUID"
        );
        assert!(
            captured.contains("[0]"),
            "prompt should contain integer ID [0]"
        );
    }

    #[tokio::test]
    async fn test_support_decision() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User likes coffee",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User likes coffee".to_string();
        existing.confidence = 0.5;
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let support_response = r#"{"decisions":[{"action":"SUPPORT","fact_index":0,"match_index":0,"context_label":"work","reason":"reinforces coffee preference"}]}"#;
        let llm = Arc::new(MockLlm::new(support_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact(
            "User drinks coffee at the office daily",
            "preferences",
        )];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);

        let updated = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert!((updated.confidence - 0.6).abs() < f32::EPSILON);
        assert_eq!(updated.relations.len(), 1);
        assert_eq!(updated.relations[0].relation_type, RelationType::Supports);
        assert_eq!(updated.relations[0].context_label.as_deref(), Some("work"));
    }

    #[tokio::test]
    async fn test_contextualize_decision() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User likes coffee",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User likes coffee".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let ctx_response = r#"{"decisions":[{"action":"CONTEXTUALIZE","fact_index":0,"match_index":0,"context_label":"evening","reason":"adds situational nuance"}]}"#;
        let llm = Arc::new(MockLlm::new(ctx_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("User prefers tea in the evening", "preferences")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].l0_abstract, "User prefers tea in the evening");
        assert_eq!(result[0].relations.len(), 1);
        assert_eq!(
            result[0].relations[0].relation_type,
            RelationType::Contextualizes
        );
        assert_eq!(result[0].relations[0].target_id, existing.id);
        assert_eq!(
            result[0].relations[0].context_label.as_deref(),
            Some("evening")
        );
    }

    #[tokio::test]
    async fn test_contradict_temporal_routes_to_supersede() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User prefers Python",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User prefers Python".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let contradict_response = r#"{"decisions":[{"action":"CONTRADICT","fact_index":0,"match_index":0,"reason":"now prefers Rust"}]}"#;
        let llm = Arc::new(MockLlm::new(contradict_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact(
            "User now prefers Rust over Python",
            "preferences",
        )];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].l0_abstract, "User now prefers Rust over Python");

        let old = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert!(old.invalidated_at.is_some());
        assert_eq!(old.superseded_by.as_deref(), Some(result[0].id.as_str()));
    }

    #[tokio::test]
    async fn test_contradict_general_creates_with_evidence() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "Deployment succeeded without issues",
            Category::Patterns,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "Deployment succeeded without issues".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let contradict_response = r#"{"decisions":[{"action":"CONTRADICT","fact_index":0,"match_index":0,"reason":"deployment actually had failures"}]}"#;
        let llm = Arc::new(MockLlm::new(contradict_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("Deployment had critical failures", "patterns")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].l0_abstract, "Deployment had critical failures");
        assert_eq!(result[0].relations.len(), 1);
        assert_eq!(
            result[0].relations[0].relation_type,
            RelationType::Contradicts
        );
        assert_eq!(result[0].relations[0].target_id, existing.id);

        let old = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert!(old.invalidated_at.is_none());
        assert_eq!(old.relations.len(), 1);
        assert_eq!(old.relations[0].relation_type, RelationType::Contradicts);
        assert_eq!(old.relations[0].target_id, result[0].id);
    }

    #[tokio::test]
    async fn test_preference_slot_guard() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "喜欢星巴克的拿铁",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "喜欢星巴克的拿铁".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let llm = Arc::new(MockLlm::new("should not be called"));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("喜欢星巴克的美式", "preferences")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].l0_abstract, "喜欢星巴克的美式");
        assert_ne!(result[0].id, existing.id);
    }

    #[tokio::test]
    async fn test_category_aware_profile_merge() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User is a backend engineer",
            Category::Profile,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User is a backend engineer".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let merge_response = r#"{"decisions":[{"action":"MERGE","fact_index":0,"match_index":0,"merged_content":"User is a senior backend engineer at Stripe","reason":"profile always merges"}]}"#;
        let llm = Arc::new(MockLlm::new(merge_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact(
            "User is now a senior engineer at Stripe",
            "profile",
        )];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].content,
            "User is a senior backend engineer at Stripe"
        );

        let updated = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert_eq!(
            updated.content,
            "User is a senior backend engineer at Stripe"
        );
    }

    #[tokio::test]
    async fn test_category_aware_events_append() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "Deployed v2.0 to production on Jan 1",
            Category::Events,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "Deployed v2.0 to production on Jan 1".to_string();
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let create_response = r#"{"decisions":[{"action":"CREATE","fact_index":0,"reason":"events are append-only"}]}"#;
        let llm = Arc::new(MockLlm::new(create_response));

        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("Deployed v2.1 hotfix on Jan 5", "events")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].l0_abstract, "Deployed v2.1 hotfix on Jan 5");
        assert_ne!(result[0].id, existing.id);

        let old = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert!(old.invalidated_at.is_none());
        assert!(old.superseded_by.is_none());
    }

    #[tokio::test]
    async fn test_session_fast_merge() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User likes coffee in the morning",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User likes coffee in the morning".to_string();
        existing.session_id = Some("s-001".to_string());
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        struct PanicLlm;
        #[async_trait::async_trait]
        impl LlmService for PanicLlm {
            async fn complete_text(&self, _system: &str, _user: &str) -> Result<String, OmemError> {
                panic!("LLM should not be called when fast session merge handles the fact");
            }
        }

        let llm = Arc::new(PanicLlm);
        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("User likes morning coffee", "preferences")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, Some("s-001".to_string()))
            .await
            .expect("reconcile");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, existing.id);
        assert_eq!(result[0].l0_abstract, "User likes morning coffee");

        let updated = store
            .get_by_id(&existing.id)
            .await
            .expect("get")
            .expect("found");
        assert_eq!(updated.l0_abstract, "User likes morning coffee");
    }

    #[tokio::test]
    async fn test_fast_session_merge_no_session_id() {
        let (store, _dir) = setup().await;
        let embed = Arc::new(MockEmbed);

        let mut existing = Memory::new(
            "User likes coffee in the morning",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        existing.l0_abstract = "User likes coffee in the morning".to_string();
        existing.session_id = Some("s-001".to_string());
        store
            .create(&existing, Some(&vec![0.0; 1024]))
            .await
            .expect("create");

        let create_response = r#"{"decisions":[{"action":"CREATE","fact_index":0,"reason":"no session_id match"}]}"#;
        let llm = Arc::new(MockLlm::new(create_response));
        let reconciler = Reconciler::new(llm, store.clone(), embed);
        let facts = vec![make_fact("User likes morning coffee", "preferences")];

        let result = reconciler
            .reconcile(&facts, "t-001", None, None)
            .await
            .expect("reconcile");

        assert_eq!(result.len(), 1);
        assert_ne!(result[0].id, existing.id);
        assert_eq!(result[0].l0_abstract, "User likes morning coffee");
    }
}
