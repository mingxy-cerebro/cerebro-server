# Ingest Module

## Overview

The ingest module implements Cerebro's 11-stage memory ingestion pipeline — transforming raw conversation messages into structured, deduplicated, clustered memories. This is the "write path" of the memory system.

- **12 source files**, ~5,882 lines of Rust
- **2 modes**: `Smart` (LLM extraction + full pipeline) and `Raw` (session storage only)
- **7 reconciliation decisions**: CREATE, MERGE, SKIP, SUPERSEDE, SUPPORT, CONTEXTUALIZE, CONTRADICT

### File Inventory

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | 24 | Module declarations and re-exports |
| `pipeline.rs` | 776 | Main orchestrator: `IngestPipeline::ingest()` |
| `admission.rs` | 559 | 5-dimension quality scoring gate |
| `extractor.rs` | 308 | LLM-based fact extraction from conversations |
| `intelligence.rs` | 505 | Post-import async re-extraction (`IntelligenceTask`) |
| `noise.rs` | 351 | Regex + vector similarity noise filtering |
| `privacy.rs` | 87 | `<private>` tag redaction |
| `prompts.rs` | 804 | LLM prompt templates (extraction + reconciliation) |
| `reconciler.rs` | 1,467 | 7-decision memory reconciliation engine |
| `session.rs` | 739 | Session message storage (LanceDB) |
| `preference_slots.rs` | 171 | Brand-item preference pattern detection |
| `types.rs` | 91 | Core ingest types |

---

## Pipeline Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         IngestRequest (messages[])                          │
│                              IngestMode::Smart                              │
└─────────────────────┬───────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 0: SESSION STORAGE                                               │
  │  SessionStore::bulk_create() — dedup by content_hash                    │
  │  Raw mode returns here. Smart mode continues.                           │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 1: MESSAGE SELECTION                                             │
  │  select_messages() — last 20 msgs, 200KB byte budget                    │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 2: PRE-FILTER (meta-ops)                                         │
  │  should_skip_content() + is_meta_operation() — skip tool output, etc.   │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 3: PRIVACY STRIP                                                 │
  │  strip_private_content() — redact <private>...</private> → [REDACTED]   │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 4: FULLY-PRIVATE FILTER                                          │
  │  is_fully_private() — skip if nothing remains after strip               │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 5: LLM FACT EXTRACTION                                           │
  │  FactExtractor::extract() — up to 15 facts, max 8000 chars input        │
  │  Uses build_system_prompt() + build_user_prompt()                       │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 6: NOISE FILTER                                                  │
  │  NoiseFilter::is_noise() — regex patterns + vector prototype similarity │
  │  Learned noise vectors accumulate (max 200)                             │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 7: ADMISSION CONTROL                                             │
  │  AdmissionControl::evaluate() — 5-dimension composite score             │
  │  Presets: Balanced (0.50/0.65), Conservative (0.58/0.72), HighRecall    │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 8: PRIVACY TAGGING                                               │
  │  detect_private_content() — regex detect IPs, passwords, keys, etc.     │
  │  Sets visibility=private, owner_agent_id, adds "私密" tag               │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 9: RECONCILIATION                                                │
  │  Reconciler::reconcile() — 7 decisions against existing memories        │
  │  Exact match dedup → batch self-dedup → fuzzy pairs → LLM decisions     │
  └───────────────────┬─────────────────────────────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────────────────────────────┐
  │  STAGE 10: CLUSTER ASSIGNMENT                                           │
  │  ClusterAssigner::assign() → ClusterManager create or assign            │
  │  Embeds memory content for new cluster creation                         │
  └─────────────────────────────────────────────────────────────────────────┘
```

---

## Stage Details

### Stage 0: Session Storage
- **File**: `session.rs`
- **Purpose**: Persist raw conversation messages before any processing
- **Key Functions**: `SessionStore::bulk_create()`, `compute_content_hash()`
- **LLM**: No
- **Input**: `Vec<IngestMessage>`
- **Output**: Stored count, content-hash deduplication

### Stage 1: Message Selection
- **File**: `pipeline.rs`
- **Purpose**: Budget-aware message trimming to fit LLM context
- **Key Functions**: `select_messages()`
- **LLM**: No
- **Input**: `&[IngestMessage]`
- **Output**: `Vec<IngestMessage>` (max 20 msgs, 200KB total)
- **Notes**: Takes newest messages first, always includes at least one

### Stage 2: Pre-Filter (Meta-Operations)
- **File**: `pipeline.rs`
- **Purpose**: Filter out tool output, system messages, build logs
- **Key Functions**: `is_meta_operation()`, `should_skip_content()`
- **LLM**: No
- **Input**: `Vec<IngestMessage>`
- **Output**: Filtered messages
- **Notes**: 20-regex `RegexSet` + 12 system pattern strings (skip if 3+ match)

### Stage 3-4: Privacy Strip & Filter
- **File**: `privacy.rs`
- **Purpose**: Redact `<private>` tagged content before LLM processing
- **Key Functions**: `strip_private_content()`, `is_fully_private()`
- **LLM**: No
- **Input**: Message content strings
- **Output**: Redacted strings or skip if fully private

### Stage 5: LLM Fact Extraction
- **File**: `extractor.rs`
- **Purpose**: Extract structured facts from conversation text
- **Key Functions**: `FactExtractor::extract()`, `format_messages()`, `calculate_quality_score()`
- **LLM**: Yes — `complete_json()` with extraction prompt
- **Input**: Sanitized `Vec<IngestMessage>`
- **Output**: `Vec<ExtractedFact>` (max 15, confidence >= 3, non-empty abstract)
- **Valid Categories**: profile, preferences, entities, events, cases, patterns

### Stage 6: Noise Filter
- **File**: `noise.rs`
- **Purpose**: Remove low-quality, repetitive, or prototype-matching facts
- **Key Functions**: `NoiseFilter::is_noise()`, `NoiseFilter::learn_noise()`
- **LLM**: No
- **Input**: `ExtractedFact` + optional embedding vector
- **Output**: Filtered facts, learned noise vectors
- **Threshold**: 0.82 cosine similarity, max 200 learned vectors

### Stage 7: Admission Control
- **File**: `admission.rs`
- **Purpose**: 5-dimension quality gate for each fact
- **Key Functions**: `AdmissionControl::evaluate()`
- **LLM**: No
- **Input**: `ExtractedFact`, category, conversation text
- **Output**: `AdmissionResult` (admitted/rejected with score + audit)
- **Dimensions**:
  - Utility (W=0.15): structural quality heuristics
  - Confidence (W=0.15): Jaccard similarity to source conversation
  - Novelty (W=0.10): 1.0 - max_similarity to existing memories
  - Recency (W=0.10): time-decay of similar memories
  - Type Prior (W=0.30): category-based base score
  - Semantic Quality (W=0.20): fact structural scoring

### Stage 8: Privacy Tagging
- **File**: `pipeline.rs`
- **Purpose**: Auto-detect and tag sensitive content post-extraction
- **Key Functions**: `detect_private_content()`
- **LLM**: No
- **Input**: `ExtractedFact` fields
- **Output**: Facts with `visibility=private`, `owner_agent_id`, `"私密"` tag
- **Patterns**: IP addresses, passwords, API keys, SSH keys, DB URLs, emails, phones, cards, IDs

### Stage 9: Reconciliation
- **File**: `reconciler.rs`
- **Purpose**: Merge/update/deduplicate facts against existing memories
- **Key Functions**: `Reconciler::reconcile()`, `exact_match_dedup()`, `batch_self_dedup()`
- **LLM**: Yes — `complete_json()` with reconciliation prompt
- **Input**: Admitted facts, existing memories from vector search
- **Output**: `Vec<Memory>` (created/updated)
- **Decisions**:
  - **CREATE** — new information
  - **MERGE** — enrich existing memory (downgrades to CREATE if pinned)
  - **SKIP** — duplicate or inferior
  - **SUPERSEDE** — contradicts/updates (archives old)
  - **SUPPORT** — confirms existing (boosts confidence)
  - **CONTEXTUALIZE** — situational nuance (creates related memory)
  - **CONTRADICT** — direct contradiction
- **Category Rules**: profile always MERGE; events/cases only CREATE/SKIP; preferences/entities/patterns support all 7

### Stage 10: Cluster Assignment
- **File**: `pipeline.rs` (calls into `cluster/`)
- **Purpose**: Assign new memories to existing or new clusters
- **Key Functions**: `ClusterAssigner::assign()`, `ClusterManager::create_cluster()`
- **LLM**: No (uses embeddings)
- **Input**: `Memory`
- **Output**: Cluster link or new cluster creation

---

## Key Types

### IngestRequest / IngestResponse
```rust
pub struct IngestRequest {
    pub messages: Vec<IngestMessage>,
    pub tenant_id: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub entity_context: Option<String>,
    pub mode: IngestMode,  // Smart | Raw
}

pub struct IngestResponse {
    pub task_id: String,
    pub stored_count: usize,
}
```

### ExtractedFact
```rust
pub struct ExtractedFact {
    pub l0_abstract: String,      // One-line summary
    pub l1_overview: String,      // Short paragraph
    pub l2_content: String,       // Full detail
    pub category: String,         // profile | preferences | entities | events | cases | patterns
    pub tags: Vec<String>,
    pub source_text: Option<String>,
    pub quality_score: f32,       // 0.0-1.0 structural score
    pub visibility: String,       // global | private
    pub owner_agent_id: String,
    pub llm_confidence: u8,       // 1-5 (0 = unset, defaults to 3)
}
```

### ReconcileDecision / ReconcileResult
```rust
pub struct ReconcileDecision {
    pub action: String,           // CREATE | MERGE | SKIP | SUPERSEDE | SUPPORT | CONTEXTUALIZE | CONTRADICT
    pub fact_index: usize,
    pub match_index: Option<usize>,
    pub merged_content: Option<String>,
    pub context_label: Option<String>,
    pub reason: Option<String>,
}

pub struct ReconcileResult {
    pub decisions: Vec<ReconcileDecision>,
}
```

### AdmissionResult / AdmissionAudit
```rust
pub struct AdmissionResult {
    pub admitted: bool,
    pub score: f32,
    pub hint: String,
    pub audit: AdmissionAudit,    // per-dimension breakdown
}

pub struct AdmissionAudit {
    pub utility: f32,
    pub confidence: f32,
    pub novelty: f32,
    pub recency: f32,
    pub type_prior: f32,
    pub semantic_quality: f32,
    pub composite: f32,
    pub max_similarity: f32,
    pub reason: String,
}
```

### SessionMessage / SessionStore
```rust
pub struct SessionMessage {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub role: String,
    pub content: String,
    pub content_hash: String,
    pub tags: Vec<String>,
    pub created_at: String,
}
```

---

## LLM Integration

| Stage | Uses LLM | Prompt Function | Model |
|-------|----------|-----------------|-------|
| Fact Extraction | Yes | `build_system_prompt()`, `build_user_prompt()` | Primary LLM (`OMEM_LLM_*`) |
| Reconciliation | Yes | `build_reconcile_prompt()` | Primary LLM |
| Intelligence Task | Yes | Same as extraction + reconciliation | Primary LLM |
| Admission | No | — | Embedding only |
| Noise Filter | No | — | Embedding only |

### Prompt Characteristics
- **Extraction prompts**: `BASE_SYSTEM_PROMPT` + optional entity context (truncated to 1500 chars)
- **Reconciliation prompts**: `RECONCILE_SYSTEM_PROMPT` + new facts + existing memories + fuzzy pair hints
- **Prompt language**: Preserves input language (Chinese/English) — never forces translation

---

## Concurrency

### Semaphores
- **`import_semaphore(3)`** — limits concurrent import operations (`api/handlers/imports.rs`)
- **`reconcile_semaphore(1)`** — serializes reconciliation in `IntelligenceTask` (`intelligence.rs`)

### Spawn Patterns
- **Fast path**: `ingest()` returns immediately after session storage (STAGE 0)
- **Slow path**: `tokio::spawn()` at `pipeline.rs:133` runs stages 1-10 in background
- **IntelligenceTask**: Spawns asynchronously for post-import re-extraction

### Critical Warning
> **`pipeline.rs:133`** — The background spawn has **no semaphore**. Under high load, unlimited LLM + embedding tasks can accumulate, exhausting API rate limits and memory.

---

## Warnings

### 1. Unbounded Background Spawn (`pipeline.rs:133`)
The slow-path `tokio::spawn()` at line 133 is not bounded by any semaphore. Every Smart-mode ingest request spawns a new background task that may call LLM + embedding services. Under burst load, this can:
- Exhaust LLM API rate limits
- Accumulate unbounded memory usage from pending tasks
- Cause embedding service overload

**Mitigation**: Add a semaphore around the spawn or the LLM calls within it.

### 2. Unbounded `all_facts` Accumulation (`intelligence.rs`)
In `IntelligenceTask::run_inner()`, facts from multiple chunks are accumulated into a single `Vec` without a hard limit:
```rust
let mut all_facts = Vec::new();
all_facts.extend(facts);  // No max cap
```
For very large imports, this can grow unbounded before reconciliation.

### 3. O(n²) Fuzzy Pair Computation (`reconciler.rs:815`)
`compute_fuzzy_pairs()` compares every fact against every other fact:
```rust
fn compute_fuzzy_pairs(facts: &[ExtractedFact]) -> Vec<(usize, usize)> {
    // Double nested loop — O(n²) in fact count
}
```
With large batches of admitted facts, this becomes expensive.

### 4. Large Prompt Strings (`reconciler.rs`, `prompts.rs`)
The reconciliation prompt builds a large string containing:
- All new facts
- Up to 150 existing memories
- Fuzzy pair annotations
- Full system prompt (~2KB)

This can exceed LLM context windows for large batches or long memory content.

### 5. Regex Compilation (`extractor.rs`, `noise.rs`, `pipeline.rs`)
Multiple regex patterns are compiled with `Regex::new()` at runtime inside functions. These should use `LazyLock` or `OnceLock` for reuse.

---

## Testing

### Test Patterns Found

**`pipeline.rs` — 13 tests:**
- `test_fast_path_stores_sessions` — verifies session storage + count
- `test_content_hash_dedup_via_pipeline` — dedup via same content hash
- `test_message_budget` — enforces MESSAGE_BUDGET (20)
- `test_message_budget_byte_limit` — enforces BYTE_BUDGET (200KB)
- `test_message_budget_always_includes_one` — always at least 1 message
- `test_message_budget_empty` — empty input handling
- `test_message_budget_preserves_order` — chronological order
- `test_raw_mode` — no LLM calls in Raw mode
- `test_graceful_degradation` — succeeds even when LLM fails
- `test_empty_messages_rejected` — validation error
- `test_auto_generated_session_id` — UUID auto-generation
- `test_detect_private_content_*` — 4 tests for IP, password, API key, personal data

**`privacy.rs` — 6 tests:**
- `test_strip_private`, `test_strip_private_multiline`, `test_strip_private_case_insensitive`, `test_strip_multiple_private_sections`, `test_no_private_tags`, `test_fully_private`

**Other files** (`admission.rs`, `extractor.rs`, `noise.rs`, `session.rs`, `reconciler.rs`) contain additional inline unit tests using mock LLM and embedding services.

### Mock Patterns
- `MockEmbed` — returns 1024-dim zero vectors
- `TrackingLlm` — counts calls, returns configured JSON
- `FailingLlm` — always returns `OmemError::Llm`

---

## Module Reference

| Module | Exports |
|--------|---------|
| `admission` | `AdmissionControl`, `AdmissionPreset`, `AdmissionResult`, `AdmissionAudit` |
| `extractor` | `FactExtractor` |
| `intelligence` | `IntelligenceTask`, `ContentHint`, `detect_content_type()` |
| `noise` | `NoiseFilter`, `cosine_similarity()`, `NOISE_PROTOTYPE_TEXTS` |
| `pipeline` | `IngestPipeline` |
| `preference_slots` | `infer_preference_slot()`, `PreferenceSlot` |
| `privacy` | `strip_private_content()`, `is_fully_private()` |
| `reconciler` | `Reconciler` |
| `session` | `SessionStore`, `SessionMessage` |
| `types` | `ExtractedFact`, `ExtractionResult`, `IngestRequest`, `IngestResponse`, `ReconcileDecision`, `ReconcileResult` |

---

*License: Apache-2.0*
