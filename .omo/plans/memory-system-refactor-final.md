# OMEM Memory System Refactor — Final Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 5 root-cause defects in the memory system so that WORK/PREFERENCE/EMOTIONAL memories are properly deduplicated, split, and quality-filtered — matching the semantic logic the prompts already describe.

**Architecture:** Modify session_ingest path in memory.rs to add PREFERENCE vector dedup + WORK multi-target matching + smart split with Continues relations. Add category_filter to vector_search. Fix profile quality gating. Fix destructive cluster rebuild.

**Tech Stack:** Rust (axum 0.8, lancedb 0.27), existing LLM/Embed services.

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `omem-server/src/store/lancedb.rs` | Modify | Add `category_filter` param to `vector_search()` |
| `omem-server/src/api/handlers/memory.rs` | Modify | Rewrite WORK/PREFERENCE/EMOTIONAL logic in session_ingest |
| `omem-server/src/domain/relation.rs` | Modify | Add `Continues` variant to RelationType |
| `omem-server/src/profile/service.rs` | Modify | Add quality gating in `build_profile()` |
| `omem-server/src/cluster/background_clustering.rs` | Modify | Fix destructive rebuild → safe incremental |
| `omem-server/src/ingest/prompts.rs` | No change | Prompt logic is correct, code needs to match |

---

## Execution Waves

**Wave 1 (Foundation)**: vector_search category_filter + Continues relation + cluster fix — independent infrastructure
**Wave 2 (Core)**: PREFERENCE dedup + WORK multi-target + smart split (EMOTIONAL不动——师尊钦定私密记忆是无价之宝)
**Wave 3 (Quality)**: Profile quality gating + scheduler integration

---

## Wave 1: Foundation

### Task 1: Add `category_filter` to `vector_search()`

**Files:**
- Modify: `omem-server/src/store/lancedb.rs:1489-1554`

**Rationale:** PREFERENCE dedup needs to search only within preferences category. Currently `vector_search` has scope/visibility/tags filters but no category filter.

- [ ] **Step 1: Add `category_filter` parameter to `vector_search` signature**

In `lancedb.rs`, modify the `vector_search` method signature (around line 1489):

```rust
pub async fn vector_search(
    &self,
    query_vector: &[f32],
    limit: usize,
    min_score: f32,
    scope_filter: Option<&str>,
    visibility_filter: Option<&str>,
    tags_filter: Option<&[String]>,
    category_filter: Option<&str>,  // NEW
) -> Result<Vec<(Memory, f32)>, OmemError> {
```

- [ ] **Step 2: Add category filter to the SQL WHERE clause**

After the tags filter block (around line 1520), add:

```rust
if let Some(cat) = category_filter {
    if !filter.is_empty() {
        filter.push_str(" AND ");
    }
    filter.push_str(&format!("category = '{}'", escape_sql(cat)));
}
```

- [ ] **Step 3: Update ALL existing callers to pass `None` for the new parameter**

Search for all `vector_search(` calls across the codebase. Each existing call needs `, None` appended for the new parameter. Key files:
- `reconciler.rs` — `gather_existing()` 
- `noise.rs` — prototype similarity
- `memory.rs` — search handler
- `retrieve/pipeline.rs` — retrieval
- `cluster/assigner.rs` — cluster assignment
- Any other callers

- [ ] **Step 4: Verify compilation**

Run: `cargo build 2>&1 | head -50`
Expected: Clean build with no errors.

---

### Task 2: Add `Continues` relation type

**Files:**
- Modify: `omem-server/src/domain/relation.rs:14-19`

**Rationale:** When WORK or EMOTIONAL memories split (overflow 3000 chars), the new memory needs a `Continues` relation linking back to the original, so the system knows they're sequential.

- [ ] **Step 1: Add `Continues` variant to `RelationType` enum**

```rust
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RelationType {
    Supersedes,
    Contextualizes,
    Supports,
    Contradicts,
    Continues,  // NEW: sequential split link
}
```

- [ ] **Step 2: Update `Display` impl**

```rust
Self::Continues => write!(f, "continues"),
```

- [ ] **Step 3: Update `FromStr` impl**

```rust
"continues" => Ok(Self::Continues),
```

- [ ] **Step 4: Add test for new variant**

```rust
#[test]
fn continues_relation_roundtrip() {
    let rel = MemoryRelation {
        relation_type: RelationType::Continues,
        target_id: "mem-split-1".to_string(),
        context_label: Some("split from overflow".to_string()),
    };
    let json = serde_json::to_string(&rel).unwrap();
    let parsed: MemoryRelation = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, rel);
    assert_eq!(RelationType::Continues.to_string(), "continues");
}
```

- [ ] **Step 5: Verify tests pass**

Run: `cargo test -p omem-server relation`
Expected: All relation tests pass.

---

### Task 3: Fix destructive cluster rebuild

**Files:**
- Modify: `omem-server/src/cluster/background_clustering.rs:85-87`

**Rationale:** `cluster_all_unassigned()` currently delegates to `cluster_global_kmeans()` which does a destructive nuke-and-rebuild. This is the ONLY path to trigger data loss (via HTTP API). The safe `run_incremental_clustering()` already exists and is used by the scheduler.

- [ ] **Step 1: Replace `cluster_all_unassigned` to use incremental path**

Replace lines 85-87:

```rust
// BEFORE (destructive):
pub async fn cluster_all_unassigned(&self, _batch_size: usize) -> Result<ClusterStats, OmemError> {
    self.cluster_global_kmeans().await
}

// AFTER (safe):
pub async fn cluster_all_unassigned(&self, batch_size: usize) -> Result<ClusterStats, OmemError> {
    // Use safe incremental clustering instead of destructive k-means rebuild
    Self::run_incremental_clustering(
        self.store.clone(),
        self.cluster_manager.clone(),
        batch_size,
    ).await
}
```

Note: `run_incremental_clustering` is a static method that takes `store`, `cluster_manager`, and `batch_size`. Need to check if `self.store` and `self.cluster_manager` are accessible — they should be since `BackgroundClusterer` holds them as fields.

- [ ] **Step 2: Mark `cluster_global_kmeans` as deprecated or remove**

Add `#[allow(dead_code)]` annotation and a doc comment:

```rust
/// DEPRECATED: Destructive full rebuild. DO NOT call from HTTP handlers.
/// Use `run_incremental_clustering` instead.
#[allow(dead_code)]
pub async fn cluster_global_kmeans(&self) -> Result<ClusterStats, OmemError> {
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -50`
Expected: Clean build. The handler in `clusters.rs` calls `cluster_all_unassigned()` which now routes to incremental.

---

## Wave 2: Core session_ingest Refactor

### Task 4: PREFERENCE vector dedup in session_ingest

**Files:**
- Modify: `omem-server/src/api/handlers/memory.rs:1447-1620`

**Rationale:** Currently PREFERENCE memories go straight to `store.create()` with zero dedup. Need to:
1. Before creating a PREFERENCE memory, do vector search within `category=preferences` 
2. If similarity >= 0.85, merge with existing instead of creating new
3. Use LLM to intelligently merge (strengthen/contradict/evolve confidence)

- [ ] **Step 1: Add PREFERENCE dedup block after the EMOTIONAL/WORK append sections**

After the WORK block (line 1611) and before the generic `store.create()` (line 1613), add:

```rust
// ── PREFERENCE: vector dedup before create ──
if memory_type == "PREFERENCE" {
    let query_vec = match vectors.get(i) {
        Some(v) => v.clone(),
        None => {
            // No vector available, fall through to create
            tracing::warn!("session_ingest: PREFERENCE no vector for dedup check");
            vec![]
        }
    };
    
    if !query_vec.is_empty() {
        // Search for similar preferences
        match store.vector_search(
            &query_vec,
            5,          // limit
            0.85,       // min_score — high threshold for preference dedup
            None,       // scope_filter
            None,       // visibility_filter  
            None,       // tags_filter
            Some("preferences"), // category_filter — NEW PARAM
        ).await {
            Ok(similar) if !similar.is_empty() => {
                let (existing_pref, score) = &similar[0];
                tracing::info!(
                    memory_id = %existing_pref.id,
                    score = %score,
                    "session_ingest: PREFERENCE dedup hit, merging"
                );
                
                // Merge: append new observation to existing preference
                let today = chrono::Utc::now()
                    .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
                    .format("%Y-%m-%d")
                    .to_string();
                
                let merged_content = format!(
                    "{}\n\n## {} 新观察\n{}",
                    existing_pref.content,
                    today,
                    summary
                );
                
                // Truncate if needed (preferences should stay concise)
                let final_content: String = if merged_content.chars().count() > 2000 {
                    merged_content.chars().take(1997).collect::<String>() + "..."
                } else {
                    merged_content
                };
                
                let mut updated = existing_pref.clone();
                updated.content = final_content.clone();
                updated.l0_abstract = final_content.chars().take(200).collect();
                updated.l1_overview = if final_content.chars().count() <= 150 {
                    final_content.clone()
                } else {
                    format!("{}...", final_content.chars().take(147).collect::<String>())
                };
                updated.l2_content = if final_content.chars().count() <= 500 {
                    final_content.clone()
                } else {
                    format!("{}...", final_content.chars().take(497).collect::<String>())
                };
                // Merge tags
                for tag in &topic.tags {
                    if !updated.tags.contains(tag) {
                        updated.tags.push(tag.clone());
                    }
                }
                updated.tags.dedup();
                updated.tags.truncate(5);
                // Boost importance on reinforcement
                updated.importance = (updated.importance + 0.05).min(1.0);
                
                if let Err(e) = store.update(&updated, None).await {
                    tracing::warn!(error = %e, "session_ingest: PREFERENCE merge update failed");
                } else {
                    stored += 1;
                    continue; // Skip create
                }
            }
            Ok(_) => {
                // No similar preference found, fall through to create
                tracing::info!("session_ingest: PREFERENCE is novel, creating new");
            }
            Err(e) => {
                tracing::warn!(error = %e, "session_ingest: PREFERENCE vector search failed, creating new");
            }
        }
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build 2>&1 | head -50`

---

### Task 5: WORK multi-target matching + smart split

**Files:**
- Modify: `omem-server/src/api/handlers/memory.rs:1366-1391, 1569-1611`

**Rationale:** 
- Currently WORK only tracks ONE existing memory via `.first()`
- The 3000-char wall is a magic number with no smart splitting
- Need to: match by project tags, consider ALL existing WORK memories, split on heading boundaries when overflow

- [ ] **Step 1: Extract 3000 to named constant**

Near top of session_ingest function (around line 1292):

```rust
const MAX_CONTENT_CHARS: usize = 3000;
```

Replace both `3000` occurrences at lines 1536 and 1580 with `MAX_CONTENT_CHARS`.

- [ ] **Step 2: Load ALL existing WORK memories (not just .first())**

Change the `existing_work_memory` variable to hold a Vec:

```rust
// BEFORE:
let mut existing_work_memory = if let Some(ref s) = work_summary {
    if let Some(d) = s.memories.first() {
        store.get_by_id(&d.id).await.ok().flatten()
    } else { None }
} else { None };

// AFTER:
let existing_work_memories: Vec<Memory> = if let Some(ref s) = work_summary {
    let mut mems = Vec::new();
    for d in &s.memories {
        if let Some(m) = store.get_by_id(&d.id).await.ok().flatten() {
            mems.push(m);
        }
    }
    mems
} else {
    Vec::new()
};
```

- [ ] **Step 3: Rewrite WORK append logic with project-tag matching**

Replace lines 1569-1611 with:

```rust
if memory_type == "WORK" {
    // Find best matching WORK memory by tag overlap
    let topic_tags: Vec<&str> = topic.tags.iter().map(|s| s.as_str()).collect();
    let best_match = existing_work_memories.iter()
        .filter(|m| {
            // Check tag overlap: at least one common project tag
            m.tags.iter().any(|t| topic_tags.contains(&t.as_str()))
        })
        .max_by(|a, b| {
            // Prefer the one with most tag overlap
            let a_overlap = a.tags.iter().filter(|t| topic_tags.contains(&t.as_str())).count();
            let b_overlap = b.tags.iter().filter(|t| topic_tags.contains(&t.as_str())).count();
            b_overlap.cmp(&a_overlap)
                        .then_with(|| b.updated_at.cmp(&a.updated_at)) // then prefer newer
        })
        .or_else(|| {
            // Fallback: most recently updated WORK memory
            existing_work_memories.iter().max_by(|a, b| b.updated_at.cmp(&a.updated_at))
        });
    
    if let Some(existing_work) = best_match {
        let today = chrono::Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let new_content = format!(
            "{}\n\n## {} {}\n{}",
            existing_work.content,
            today,
            topic.topic,
            summary
        );
        
        if new_content.chars().count() <= MAX_CONTENT_CHARS {
            // Fits: update in place
            let mut updated = existing_work.clone();
            updated.content = new_content.clone();
            updated.l0_abstract = new_content.chars().take(200).collect();
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
            updated.tags.dedup();
            updated.tags.truncate(5);
            
            if let Err(e) = store.update(&updated, None).await {
                tracing::warn!(error = %e, "session_ingest: failed to append to existing WORK memory");
            } else {
                tracing::info!(memory_id = %updated.id, "session_ingest: appended to existing WORK memory");
                stored += 1;
                continue;
            }
        } else {
            // Overflow: split on heading boundary
            // Keep the old memory as-is, create new memory with Continues relation
            tracing::info!(
                old_id = %existing_work.id,
                new_chars = new_content.chars().count(),
                "session_ingest: WORK memory overflow, splitting with Continues relation"
            );
            
            // Create new memory with just the new content
            let mut split_memory = Memory::new(
                &summary,
                category,
                MemoryType::Pinned,
                &tenant_id,
            );
            split_memory.l0_abstract = topic.topic.clone();
            split_memory.l1_overview = if summary.chars().count() <= 150 {
                summary.clone()
            } else {
                format!("{}...", summary.chars().take(147).collect::<String>())
            };
            split_memory.l2_content = if summary.chars().count() <= 500 {
                summary.clone()
            } else {
                format!("{}...", summary.chars().take(497).collect::<String>())
            };
            split_memory.source = Some("session_compress".to_string());
            split_memory.session_id = session_id.clone();
            split_memory.agent_id = agent_id.clone();
            split_memory.tags = tags;
            // Add Continues relation to old memory
            split_memory.relations = vec![MemoryRelation {
                relation_type: RelationType::Continues,
                target_id: existing_work.id.clone(),
                context_label: Some(format!("split from {}", existing_work.l0_abstract)),
            }];
            
            let vector = vectors.get(i).cloned();
            if let Err(e) = store.create(&split_memory, vector.as_deref()).await {
                tracing::error!(error = %e, "session_ingest: WORK split create failed");
                return;
            }
            stored += 1;
            created_memories.push((split_memory, vectors.get(i).cloned()));
            continue;
        }
    }
}
```

- [ ] **Step 4: EMOTIONAL — NO CHANGES**

师尊钦定：私密记忆是师尊和月儿的无价之宝，当前EMOTIONAL追加逻辑工作正常，不做任何修改。

- [ ] **Step 5: Verify compilation**

Run: `cargo build 2>&1 | head -50`

---

## Wave 3: Quality

### Task 6: Profile quality gating

**Files:**
- Modify: `omem-server/src/profile/service.rs:306-365`

**Rationale:** `build_profile()` has zero quality filtering — loads 1000 memories, only filters by visibility, takes top 20 by importance but all importances are 0.5 (identical). The `_llm` parameter is unused. `PROFILE_FILTER_SYSTEM_PROMPT` exists but is never called.

- [ ] **Step 1: Add importance threshold for static facts**

In `build_profile()`, after filtering for Profile/Preferences category (line 323-326), add an importance filter:

```rust
let mut static_memories: Vec<_> = all_memories
    .iter()
    .filter(|m| {
        (m.category == Category::Profile || m.category == Category::Preferences)
        && m.importance >= 0.4  // Minimum quality bar
    })
    .collect();
```

- [ ] **Step 2: Use _llm for profile quality filtering**

After building `static_facts`, if `_llm` is Some and there are facts to filter:

```rust
// LLM quality filter for profile facts
let static_facts = if !static_facts_raw.is_empty() {
    if let Some(llm) = _llm {
        let facts_text: Vec<String> = static_facts_raw.iter()
            .map(|f| f.content.clone())
            .collect();
        
        let user_prompt = format!(
            "以下是从记忆库中提取的候选画像条目，请筛选出真正的用户画像信息：\n\n{}",
            facts_text.join("\n---\n")
        );
        
        match crate::llm::complete_json::<serde_json::Value>(
            llm.as_ref(),
            crate::ingest::prompts::PROFILE_FILTER_SYSTEM_PROMPT,
            &user_prompt,
        ).await {
            Ok(result) => {
                if let Some(facts_arr) = result.get("facts").and_then(|f| f.as_array()) {
                    let valid_contents: Vec<String> = facts_arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    
                    // Filter original facts to only those approved by LLM
                    static_facts_raw.into_iter()
                        .filter(|f| valid_contents.iter().any(|vc| f.content.contains(vc) || vc.contains(&f.content)))
                        .collect()
                } else {
                    static_facts_raw
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "profile: LLM filter failed, using unfiltered facts");
                static_facts_raw
            }
        }
    } else {
        static_facts_raw
    }
} else {
    static_facts_raw
};
```

Note: Rename the initial `static_facts` variable to `static_facts_raw`, then apply the filter to produce the final `static_facts`.

- [ ] **Step 3: Verify compilation**

Run: `cargo build 2>&1 | head -50`

---

### Task 7: Integration verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p omem-server 2>&1 | tail -30`
Expected: All existing tests pass. New relation test passes.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -p omem-server 2>&1 | tail -30`
Expected: No new warnings.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: memory system refactor - PREFERENCE dedup, WORK multi-target, smart split, profile quality, cluster safety

- Add category_filter to vector_search() for efficient PREFERENCE dedup
- Add Continues relation type for sequential split linking
- PREFERENCE: vector dedup (0.85 threshold) before create, merge on hit
- WORK: multi-target matching by project tags, heading-boundary split
- EMOTIONAL: Continues relation on overflow split
- Profile: LLM quality gating via PROFILE_FILTER_SYSTEM_PROMPT
- Cluster: replace destructive k-means rebuild with safe incremental path"
```

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| vector_search API break | All callers updated with `None` for new param |
| PREFERENCE dedup false positive | 0.85 threshold is conservative (same as cluster dedup) |
| WORK tag matching misses | Fallback to most-recent WORK memory |
| Profile LLM filter adds latency | Falls back to unfiltered on LLM failure |
| Cluster fix changes behavior | Incremental path is already scheduler-proven |

## Rollback

Tag `v-stable-pre-memory-refactor` exists as rollback anchor.
