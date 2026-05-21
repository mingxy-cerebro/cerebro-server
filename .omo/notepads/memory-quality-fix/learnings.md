# Learnings - memory-quality-fix

## [2026-05-10] Plan Context
- Plan: 记忆质量改进 - MERGE标题修复 + 主题归类 + 提示工程
- 4 impl tasks (T1-T4), 4 final verification (F1-F4)
- T1+T2+T3 parallel (different files), T4 sequential after wave 1
- Constraints: no jaccard changes, no new LLM calls, no RECONCILE changes, no plugins, no new deps
- Last commit: 7cae5eb feat: rename plugin omem→cerebro + enhance ingest reconciler dedup logic
- Working tree: clean

## [2026-05-10] T1: strip_timestamp_prefix for paragraph_diff_merge
- **Problem**: `## 2025-05-09 14:00 修复bug` vs `## 2025-05-10 09:00 修复bug` — same text but different timestamps caused exact match failure
- **Fix**: Added `strip_timestamp_prefix()` helper that strips `YYYY-MM-DD HH:MM ` prefix, leaving plain text for comparison
- **Match logic change**: L1028 now compares stripped headings instead of raw strings
- **Newer timestamp preserved**: L1037 `merged[idx].heading = new_p.heading.clone()` ensures the newer heading replaces the older one on match
- **3 pre-existing test failures** in reconciler (test_fast_session_merge_no_session_id, test_session_fast_merge, test_uuid_to_int_mapping) — NOT caused by this change, confirmed via git stash comparison
- **48 total pre-existing test failures** across api/connectors/domain/embed/ingest modules — infrastructure tests needing external services

## T2: session_ingest topic-aware matching (2026-05-10)

### Key Code Locations
- `api/handlers/memory.rs` NOT `domain/memory.rs` — session_ingest is in the API handler layer
- SessionTopicSummary struct: L1308-1323 (has topic, summary, overview, detail, tags, scope, category, memory_type)
- apply_append closure: ~L1588-1618 (updates content, l0_abstract, l1_overview, l2_content, tags)
- EMOTIONAL path: L1620-1696 (scope=="private" branch)
- WORK path: L1700-1797 (memory_type=="WORK" branch)
- existing_emotional loaded: L1418-1436 (first memory from emotional_summary)
- existing_work_memory loaded: L1428-1436 (first memory from work_summary)

### Topic Matching Pattern
- Section header format: `## {timestamp} {topic.topic}`
- Match check: `line.starts_with("## ") && line.ends_with(&topic.topic)` 
- Also fallback to `l0_abstract == topic.topic` for EMOTIONAL (l0_abstract stores last topic title)
- WORK uses only section header matching (already had section-based replace logic)

### Before Fix
- EMOTIONAL: all private emotions merged into one memory regardless of topic
- WORK: all work content merged into one memory, only replacing same-topic sections but appending different topics

### After Fix
- Both EMOTIONAL and WORK: check topic match before appending
- Same topic → merge/update (existing behavior)
- Different topic → skip append, fall through to create new memory
- Fallback shortest-fit also filtered by topic match
- Topic mismatch logged with both existing and new topic names

### Pre-existing Test Failures
- 48 tests fail on main branch (pre-existing), 368 pass
- memory handler has no dedicated unit tests
- Changes verified: cargo check passes, no new test failures introduced

## Task: BASE_SYSTEM_PROMPT WHY + ACTIONABLE_RULES (completed)
- Added `why` field to JSON output format (L446), format spec (L436), and all 3 examples
- Added `## ACTIONABLE_RULES` section before prompt close with 4 rules: conflict detection, time-sensitive marking, language fidelity, actionable over vague
- `cargo check` passes clean
- RECONCILE_SYSTEM_PROMPT untouched (confirmed via grep)

## T4: Build + Deploy (2026-05-10)

### Results
- **cargo test**: 368 passed, 48 failed (all pre-existing, need external services)
- **cargo clippy**: 3 pre-existing errors (regex look-ahead in extractor.rs), 9 warnings — 0 new from our changes
- **cargo build --release**: Success, 232M binary, md5=e3c72b6e55288c256bbc59486c79492c
- **SCP + MD5 verify**: ✅ md5 matched on server
- **systemctl restart**: active (running), PID 562702, memory 34.9M
- **health check**: `https://www.mengxy.cc/health` → `{"status":"ok"}`
- **logs**: No panic/fatal, only WARN for empty space tables (normal)
