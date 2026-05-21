# Phase 2: Categories Dictionary Table

## TL;DR

> **Quick Summary**: Replace hardcoded categories with SQLite-backed configurable system. Each tenant gets own category set, seeded with 9 design-doc categories. CRUD API + in-memory cache + dynamic LLM prompt generation.
> 
> **Deliverables**:
> - SQLite `categories` + `category_aliases` tables (tenant-isolated)
> - `SqliteStore` with `Mutex<Connection>` + `spawn_blocking`
> - `CategoryRegistry` in-memory cache (startup load + CRUD refresh)
> - CRUD API endpoints (`/v1/categories`)
> - Dynamic prompt generation replacing 4 hardcoded prompt sections
> - Updated weight functions (`category_prior`, `category_importance`) reading from DB
> - Seed data migration for new tenants (9 categories from design doc)
> 
> **Estimated Effort**: Medium (7-10 files, ~800 lines new code)
> **Parallel Execution**: YES - 3 waves
> **Critical Path**: Task 1 → Task 2 → Task 4 → Task 6 → Task 9 → F1-F4

---

## Context

### Original Request
Make hardcoded categories configurable via Web CRUD. Add new category → LLM uses its description for accurate classification.

### Interview Summary
**Key Discussions**:
- Tenant-isolated: Each tenant gets own categories, seeded on creation
- Seed data: 9 categories from design doc (preferences, identity, emotional, project, work, lessons_learned, decisions, success_patterns, mistakes)
- OLD 6 categories (profile, entities, events, cases, patterns) → completely deprecated
- No backward compatibility needed — user will use new API key after deployment
- Loading: Startup full load + CRUD refresh (in-memory DashMap cache)
- Testing: Tests-after

**Research Findings**:
- 11 MUST-CHANGE locations across 6 files mapped by explore agent
- Zero existing SQLite usage — all storage is LanceDB
- rusqlite not in Cargo.toml — needs to be added
- AppState has 16 fields (including profile_cache, ingest_semaphore), SqliteStore fits between SpaceStore and EmbedService in startup

### Metis Review
**Identified Gaps** (addressed):
- tenant_id needs index in categories table → Added to schema
- Cache consistency under concurrency → Use DashMap for lock-free reads, Mutex only for batch refresh
- Seed timing for existing tenants → Not needed (user using new API key)
- Category is already String newtype (Phase 0 done) → No enum→string migration needed

---

## Work Objectives

### Core Objective
Replace all hardcoded category definitions, weights, and prompt descriptions with a SQLite-backed configurable system that supports per-tenant customization and runtime CRUD operations.

### Concrete Deliverables
- `omem-server/Cargo.toml`: rusqlite dependency added
- `omem-server/src/store/sqlite.rs`: New SQLite store module
- `omem-server/src/store/sqlite_schema.rs`: Table creation + seed data SQL
- `omem-server/src/domain/category.rs`: Refactored with CategoryRegistry
- `omem-server/src/api/handlers/categories.rs`: New CRUD handler
- `omem-server/src/api/server.rs`: AppState + router updated
- `omem-server/src/main.rs`: SqliteStore init in startup sequence
- `omem-server/src/ingest/prompts.rs`: Dynamic prompt building
- `omem-server/src/ingest/extractor.rs`: normalize_category uses registry
- `omem-server/src/ingest/admission.rs`: category_prior from registry
- `omem-server/src/ingest/reconciler.rs`: category_importance from registry

### Definition of Done
- [ ] `cargo check` passes with zero errors
- [ ] `cargo test` — no new failures (existing 46 pre-existing failures acceptable)
- [ ] `curl POST /v1/categories` creates a new category
- [ ] `curl GET /v1/categories` returns tenant's categories including seed data
- [ ] New category added via API appears in LLM extraction prompts
- [ ] Each tenant has isolated categories

### Must Have
- SQLite categories table with tenant isolation
- CRUD API (GET list, GET single, POST create, PUT update, DELETE)
- In-memory cache (DashMap) loaded at startup, refreshed on mutation
- Dynamic prompt generation from DB category metadata
- Seed data: 9 design-doc categories for new tenants
- category_aliases table for flexible normalization
- category_prior and category_importance weights from DB

### Must NOT Have (Guardrails)
- Do NOT touch LanceDB vector storage logic
- Do NOT modify Memory struct serialization format
- Do NOT create Web UI (that's a separate project)
- Do NOT add scoring_weights table (deferred to Settings phase)
- Do NOT write data migration for old 6 categories
- Do NOT change sharing.rs, stats.rs aggregation logic (string-based, already dynamic)
- Do NOT add `as any` / `@ts-ignore` equivalents (Rust: no unsafe unwrap in production)
- Do NOT break musl static build (rusqlite bundled is required)

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (370 inline tests)
- **Automated tests**: YES (Tests-after)
- **Framework**: Rust inline `#[cfg(test)]` + `#[tokio::test]`
- **New tests**: SQLite store CRUD tests, CategoryRegistry cache tests, API handler tests

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **API endpoints**: Use Bash (curl) — Send requests, assert status + JSON fields
- **Rust internals**: Use Bash (cargo test) — Run specific test modules
- **SQLite operations**: Use Bash (cargo test) — Inline tests with in-memory SQLite

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - foundation):
├── Task 1: Add rusqlite dependency + SqliteStore skeleton [quick]
├── Task 2: Create sqlite_schema.rs with tables + seed data [quick]
├── Task 3: Refactor domain/category.rs → CategoryRegistry [deep]
└── Task 4: Add SqliteStore to AppState + main.rs startup [quick]

Wave 2 (After Wave 1 - core integration):
├── Task 5: Dynamic prompt generation in prompts.rs (depends: 2, 3) [deep]
├── Task 6: Update extractor.rs normalize_category (depends: 3) [quick]
├── Task 7: Update admission.rs + reconciler.rs weights (depends: 3) [quick]
├── Task 8: CRUD API handler + routes (depends: 1, 2, 3, 4) [unspecified-high]

Wave 3 (After Wave 2 - cleanup + tests):
├── Task 9: Update stats.rs get_config endpoint (depends: 8) [quick]
├── Task 10: Update memory.rs alias normalization (depends: 3) [quick]
└── Task 11: Tests — SQLite store + CategoryRegistry + API (depends: 5-10) [deep]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: Task 1 → Task 4 → Task 8 → Task 9 → F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 4 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | - | 4, 8 | 1 |
| 2 | - | 5, 8 | 1 |
| 3 | - | 5, 6, 7, 8, 10 | 1 |
| 4 | 1 | 8 | 1 |
| 5 | 2, 3 | 11 | 2 |
| 6 | 3 | 11 | 2 |
| 7 | 3 | 11 | 2 |
| 8 | 1, 2, 3, 4 | 9, 11 | 2 |
| 9 | 8 | 11 | 3 |
| 10 | 3 | 11 | 3 |
| 11 | 5-10 | F1-F4 | 3 |
| F1-F4 | 11 | user okay | FINAL |

### Agent Dispatch Summary

- **Wave 1**: 4 tasks — T1 `quick`, T2 `quick`, T3 `deep`, T4 `quick`
- **Wave 2**: 4 tasks — T5 `deep`, T6 `quick`, T7 `quick`, T8 `unspecified-high`
- **Wave 3**: 3 tasks — T9 `quick`, T10 `quick`, T11 `deep`
- **FINAL**: 4 reviews — F1 `oracle`, F2 `unspecified-high`, F3 `unspecified-high`, F4 `deep`

---

## TODOs

- [x] 1. Add rusqlite dependency + SqliteStore skeleton

  **What to do**:
  - Add `rusqlite = { version = "0.32", features = ["bundled"] }` to `omem-server/Cargo.toml`
  - Create `omem-server/src/store/sqlite.rs` with:
    ```rust
    pub struct SqliteStore {
        conn: std::sync::Mutex<rusqlite::Connection>,
    }
    ```
  - Implement `SqliteStore::new(db_path: &str) -> Result<Self, OmemError>` — opens SQLite with WAL mode
  - Implement `SqliteStore::init_tables(&self) -> Result<(), OmemError>` — delegates to schema module
  - Add `pub mod sqlite;` to `omem-server/src/store/mod.rs`

  **Must NOT do**:
  - Do NOT modify existing LanceDB store code
  - Do NOT add SqliteStore to AppState yet (Task 4 does this)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single file creation + Cargo.toml edit, well-defined pattern
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - `systematic-debugging`: Not needed — straightforward creation, no bugs to debug

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)
  - **Blocks**: Tasks 4, 8
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `omem-server/src/store/tenant.rs` — Two-step init pattern: `new()` + `init_table()`
  - `omem-server/src/store/spaces.rs` — Same pattern with multiple tables

  **API/Type References**:
  - `omem-server/src/domain/error.rs:OmemError` — Use `OmemError::Storage(msg)` for SQLite errors

  **WHY Each Reference Matters**:
  - tenant.rs/spaces.rs: Shows exact pattern to follow for store initialization
  - OmemError: All errors must be typed, not anyhow

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: SqliteStore creates DB file with WAL mode
    Tool: Bash (cargo test)
    Preconditions: Clean build
    Steps:
      1. cargo test -p omem-server store::sqlite::tests -- --test-threads=1
    Expected Result: All tests pass, SqliteStore::new() creates file with WAL pragma
    Failure Indicators: Compile errors, test failures
    Evidence: .sisyphus/evidence/task-1-sqlite-init.txt

  Scenario: SqliteStore init_tables creates categories table
    Tool: Bash (cargo test)
    Preconditions: SqliteStore created
    Steps:
      1. Write inline test: create store, call init_tables, query sqlite_master for 'categories' table
    Expected Result: categories table exists with correct columns
    Failure Indicators: Table not found, missing columns
    Evidence: .sisyphus/evidence/task-1-sqlite-schema.txt
  ```

  **Commit**: YES (groups with Wave 1)
  - Message: `feat(store): add SqliteStore with rusqlite dependency`
  - Files: `Cargo.toml, src/store/sqlite.rs, src/store/mod.rs`
  - Pre-commit: `cargo check`

- [x] 2. Create sqlite_schema.rs with tables + seed data

  **What to do**:
  - Create `omem-server/src/store/sqlite_schema.rs` with:
    - `CREATE_TABLES_SQL` — categories + category_aliases DDL
    - `SEED_CATEGORIES_SQL` — 9 INSERT statements for design-doc categories
    - `seed_default_categories(conn, tenant_id)` function
  - Schema (tenant-isolated):
    ```sql
    CREATE TABLE IF NOT EXISTS categories (
        name TEXT NOT NULL,
        tenant_id TEXT NOT NULL,
        display_name TEXT NOT NULL,
        description TEXT NOT NULL,
        decision_rule TEXT,
        always_merge BOOLEAN DEFAULT FALSE,
        append_only BOOLEAN DEFAULT FALSE,
        temporal_versioned BOOLEAN DEFAULT FALSE,
        merge_supported BOOLEAN DEFAULT FALSE,
        admission_weight REAL DEFAULT 0.50,
        importance_base REAL DEFAULT 0.50,
        prompt_format TEXT,
        default_visibility TEXT DEFAULT 'global',
        default_scope TEXT DEFAULT 'global',
        default_ttl_days INTEGER,
        sort_order INTEGER DEFAULT 0,
        is_active BOOLEAN DEFAULT TRUE,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (name, tenant_id)
    );
    CREATE INDEX IF NOT EXISTS idx_categories_tenant ON categories(tenant_id);

    CREATE TABLE IF NOT EXISTS category_aliases (
        alias TEXT NOT NULL,
        tenant_id TEXT NOT NULL,
        target TEXT NOT NULL,
        PRIMARY KEY (alias, tenant_id)
    );
    ```
  - Seed data (9 categories from design doc):
    | name | display_name | description | admission_weight | importance_base | behavior flags |
    |------|-------------|-------------|-----------------|----------------|---------------|
    | preferences | 偏好 | User likes/dislikes/tool choices | 0.90 | 0.70 | temporal,merge |
    | identity | 身份规则 | Stable identity traits, repeated characteristics | 0.75 | 0.80 | always_merge |
    | emotional | 感情记忆 | Emotional states and feelings | 0.65 | 0.55 | append_only |
    | project | 项目上下文 | Project-specific context and status | 0.70 | 0.60 | temporal,merge |
    | work | 工作记忆 | Work-related memories with decay | 0.55 | 0.45 | append_only,ttl=90 |
    | lessons_learned | 经验教训 | Lessons learned from experience | 0.85 | 0.70 | merge_supported |
    | decisions | 重要决策 | Important decisions made | 0.80 | 0.75 | append_only |
    | success_patterns | 成功方案 | Successful patterns and solutions | 0.85 | 0.65 | merge_supported |
    | mistakes | 犯过的错 | Mistakes and failures | 0.80 | 0.60 | merge_supported |
  - Add `pub mod sqlite_schema;` to `omem-server/src/store/mod.rs`

  **Must NOT do**:
  - Do NOT include old 6 categories (profile, entities, events, cases, patterns)
  - Do NOT add scoring_weights table

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single file with SQL constants + seed function, well-defined schema
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)
  - **Blocks**: Tasks 5, 8
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `docs/superpowers/specs/2026-05-15-memory-system-rewrite-design.md` L196-242 — Original schema design

  **API/Type References**:
  - `omem-server/src/ingest/admission.rs:278-288` — Current weights to map to admission_weight column
  - `omem-server/src/ingest/reconciler.rs:1087-1099` — Current weights to map to importance_base column

  **WHY Each Reference Matters**:
  - Design doc: Authoritative schema definition
  - admission.rs/reconciler.rs: Must match current weight values for parity

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Schema creates both tables
    Tool: Bash (cargo test)
    Steps:
      1. Test: create in-memory SQLite, run CREATE_TABLES_SQL, verify both tables exist
    Expected Result: categories and category_aliases tables created
    Evidence: .sisyphus/evidence/task-2-schema-create.txt

  Scenario: Seed data inserts 9 categories for a tenant
    Tool: Bash (cargo test)
    Steps:
      1. Test: create tables, call seed_default_categories("tenant-123"), query count
    Expected Result: 9 rows with tenant_id="tenant-123"
    Evidence: .sisyphus/evidence/task-2-seed-data.txt

  Scenario: Seed is idempotent (INSERT OR IGNORE)
    Tool: Bash (cargo test)
    Steps:
      1. Call seed_default_categories twice with same tenant_id
    Expected Result: Still 9 rows, no duplicates
    Evidence: .sisyphus/evidence/task-2-seed-idempotent.txt
  ```

  **Commit**: YES (groups with Wave 1)
  - Message: `feat(store): add SQLite schema with categories seed data`
  - Files: `src/store/sqlite_schema.rs, src/store/mod.rs`

- [x] 3. Refactor domain/category.rs → CategoryRegistry

  **What to do**:
  - Remove hardcoded string constants (`PROFILE`, `PREFERENCES`, etc.)
  - Remove hardcoded shorthand constructors (`profile()`, `preferences()`, etc.)
  - Remove `known_variants()` static method
  - Remove 4 hardcoded behavior predicates (`is_always_merge`, `is_append_only`, etc.)
  - Add `CategoryConfig` struct:
    ```rust
    pub struct CategoryConfig {
        pub name: String,
        pub display_name: String,
        pub description: String,
        pub decision_rule: Option<String>,
        pub always_merge: bool,
        pub append_only: bool,
        pub temporal_versioned: bool,
        pub merge_supported: bool,
        pub admission_weight: f32,
        pub importance_base: f32,
        pub prompt_format: Option<String>,
        pub default_visibility: String,
        pub default_scope: String,
        pub default_ttl_days: Option<i32>,
        pub sort_order: i32,
        pub is_active: bool,
    }
    ```
  - Add `CategoryRegistry` struct:
    ```rust
    pub struct CategoryRegistry {
        categories: DashMap<String, CategoryConfig>,  // key: tenant_id
        aliases: DashMap<String, Vec<(String, String)>>,  // key: tenant_id, value: (alias, target) pairs
        sqlite: Arc<SqliteStore>,
    }
    ```
  - Actually, better: per-tenant cache:
    ```rust
    pub struct CategoryRegistry {
        cache: DashMap<String, Vec<CategoryConfig>>,  // key: tenant_id
        aliases: DashMap<String, HashMap<String, String>>,  // key: tenant_id, value: alias→target
        sqlite: Arc<SqliteStore>,
    }
    ```
  - Implement:
    - `CategoryRegistry::new(sqlite: Arc<SqliteStore>) -> Self`
    - `load_for_tenant(&self, tenant_id: &str)` — loads from SQLite, caches in DashMap
    - `get_categories(&self, tenant_id: &str) -> Vec<CategoryConfig>`
    - `get_active_categories(&self, tenant_id: &str) -> Vec<CategoryConfig>`
    - `find_by_name(&self, tenant_id: &str, name: &str) -> Option<CategoryConfig>`
    - `normalize(&self, tenant_id: &str, raw: &str) -> Option<String>` — resolves aliases
    - `get_prior(&self, tenant_id: &str, name: &str) -> f32`
    - `get_importance(&self, tenant_id: &str, name: &str) -> f32`
    - `seed_tenant(&self, tenant_id: &str)` — calls sqlite_schema::seed_default_categories
    - CRUD methods: `create_category`, `update_category`, `delete_category`, `list_categories`
    - `invalidate(&self, tenant_id: &str)` — drops cache entry, next access reloads
  - Keep `Category` newtype as-is (String wrapper)
  - Keep `FromStr` impl as-is (always succeeds with lowercase)
  - Keep `Display` impl as-is

  **Must NOT do**:
  - Do NOT change Category's serde behavior (transparent String)
  - Do NOT break FromStr (always Ok)
  - Do NOT modify Memory struct

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Core domain refactor, multiple interdependent methods, needs careful design
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)
  - **Blocks**: Tasks 5, 6, 7, 8, 10
  - **Blocked By**: None (can start immediately, but references SqliteStore types — if Task 1 not done, use placeholder imports)

  **References**:

  **Pattern References**:
  - `omem-server/src/domain/category.rs` L1-151 — Current implementation to refactor
  - `omem-server/src/store/manager.rs` — LRU cache pattern with DashMap/Mutex

  **API/Type References**:
  - `omem-server/src/ingest/admission.rs:278-288` — category_prior values to preserve
  - `omem-server/src/ingest/reconciler.rs:1087-1099` — category_importance values to preserve
  - `omem-server/src/ingest/prompts.rs:368-376` — Category descriptions to move to DB

  **WHY Each Reference Matters**:
  - category.rs: The source of truth being refactored
  - store/manager.rs: Caching pattern to follow
  - admission.rs/reconciler.rs: Weight values must match seed data
  - prompts.rs: Descriptions must move to DB

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: CategoryRegistry loads and caches categories for a tenant
    Tool: Bash (cargo test)
    Steps:
      1. Create in-memory SQLite, seed tenant, create CategoryRegistry
      2. Call get_categories("tenant-1")
      3. Verify 9 categories returned
    Expected Result: 9 CategoryConfig structs with correct weights
    Evidence: .sisyphus/evidence/task-3-registry-load.txt

  Scenario: normalize() resolves aliases correctly
    Tool: Bash (cargo test)
    Steps:
      1. Seed tenant with default aliases
      2. Call normalize("tenant-1", "knowledge") → should resolve
      3. Call normalize("tenant-1", "unknown_category") → should return None
    Expected Result: Known aliases resolve, unknown returns None
    Evidence: .sisyphus/evidence/task-3-normalize-alias.txt

  Scenario: CRUD operations work and refresh cache
    Tool: Bash (cargo test)
    Steps:
      1. Create category, verify it appears in get_categories
      2. Update display_name, verify change
      3. Delete, verify removed
    Expected Result: All CRUD operations work, cache stays consistent
    Evidence: .sisyphus/evidence/task-3-crud-refresh.txt
  ```

  **Commit**: YES (groups with Wave 1)
  - Message: `refactor(category): replace hardcoded constants with CategoryRegistry`
  - Files: `src/domain/category.rs`
  - Pre-commit: `cargo check`

- [x] 4. Add SqliteStore + CategoryRegistry to AppState + main.rs startup

  **What to do**:
  - Add to `AppState` in `api/server.rs`:
    ```rust
    pub sqlite_store: Arc<SqliteStore>,
    pub category_registry: Arc<CategoryRegistry>,
    ```
  - Update `main.rs` startup sequence:
    ```rust
    // Between SpaceStore and EmbedService:
    let sqlite_path = config.store_uri().replace("lance://", "./omem-data") + "/_system/omem.db";
    let sqlite_store = Arc::new(SqliteStore::new(&sqlite_path)?);
    sqlite_store.init_tables()?;
    let category_registry = Arc::new(CategoryRegistry::new(sqlite_store.clone()));
    ```
  - Pass both into AppState assembly
  - Update `build_router()` if needed for new routes

  **Must NOT do**:
  - Do NOT change existing store initialization order
  - Do NOT break existing AppState fields

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Small mechanical changes to 2 files, following established patterns
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Task 8
  - **Blocked By**: Task 1 (SqliteStore type needed)

  **References**:

  **Pattern References**:
  - `omem-server/src/main.rs` L1-100 — Current startup sequence, insert between SpaceStore and EmbedService
  - `omem-server/src/api/server.rs` L20-40 — AppState struct definition

  **WHY Each Reference Matters**:
  - main.rs: Exact insertion point in startup
  - server.rs: AppState fields to extend

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Server starts with SqliteStore initialized
    Tool: Bash (cargo check)
    Steps:
      1. cargo check
    Expected Result: Compiles without errors
    Evidence: .sisyphus/evidence/task-4-compile.txt

  Scenario: AppState has sqlite_store and category_registry fields
    Tool: Bash (grep)
    Steps:
      1. grep for "sqlite_store" and "category_registry" in api/server.rs
    Expected Result: Both fields present in AppState
    Evidence: .sisyphus/evidence/task-4-appstate.txt
  ```

  **Commit**: YES (groups with Wave 1)
  - Message: `feat(app): add SqliteStore and CategoryRegistry to AppState`
  - Files: `src/api/server.rs, src/main.rs`

- [x] 5. Dynamic prompt generation in prompts.rs

  **What to do**:
  - Modify `build_system_prompt()` to accept `categories: &[CategoryConfig]` parameter
  - Replace hardcoded category description block (L368-376) with dynamic generation:
    ```rust
    fn build_category_section(categories: &[CategoryConfig]) -> String {
        let mut s = String::from("## Categories\nClassify each fact into exactly one category:\n\n");
        for cat in categories {
            s.push_str(&format!("- **{}**: {}", cat.name, cat.description));
            if let Some(rule) = &cat.decision_rule {
                s.push_str(&format!(" Decision: \"{}\"", rule));
            }
            s.push('\n');
        }
        s
    }
    ```
  - Replace hardcoded format rules (L385-407) — use `prompt_format` field:
    - categories with `prompt_format = "preference"` → PREFERENCE Format block
    - categories with `prompt_format = "work"` → WORK Format block
  - Modify `RECONCILE_SYSTEM_PROMPT` (L158-164) to dynamically build category rules:
    - always_merge → "profile-category memories always MERGE"
    - append_only → "X-category memories only CREATE or SKIP"
    - merge_supported → "X-category memories support all 7 operations"
  - Modify `SESSION_COMPRESS_SYSTEM_PROMPT` (L695-704) similarly
  - Modify `SESSION_EXTRACT_SYSTEM_PROMPT` (L855-917) similarly
  - Update callers in `pipeline.rs`, `extractor.rs`, `reconciler.rs` to pass categories

  **Must NOT do**:
  - Do NOT change the extraction/reconciliation logic itself, only the prompt content
  - Do NOT break language preservation rules
  - Do NOT change prompt structure (layered storage, privacy, exclusion rules)

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 4 different prompt templates to update, careful string building, must not break LLM extraction quality
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 2 + Task 3)
  - **Parallel Group**: Wave 2 (parallel with Tasks 6, 7, 8)
  - **Blocks**: Task 11
  - **Blocked By**: Tasks 2, 3

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/prompts.rs` L332-414 — BASE_SYSTEM_PROMPT with hardcoded categories
  - `omem-server/src/ingest/prompts.rs` L143-184 — RECONCILE_SYSTEM_PROMPT with category rules
  - `omem-server/src/ingest/prompts.rs` L695-704 — SESSION_COMPRESS_SYSTEM_PROMPT
  - `omem-server/src/ingest/prompts.rs` L855-917 — SESSION_EXTRACT_SYSTEM_PROMPT

  **API/Type References**:
  - `omem-server/src/domain/category.rs:CategoryConfig` — Struct with description, decision_rule, behavior flags

  **WHY Each Reference Matters**:
  - All 4 prompt locations need category descriptions replaced with dynamic content
  - CategoryConfig provides the data to build dynamic prompts

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Dynamic prompt includes all 9 seed categories
    Tool: Bash (cargo test)
    Steps:
      1. Create CategoryConfig for all 9 seed categories
      2. Call build_system_prompt() with them
      3. Assert prompt contains "preferences", "identity", "emotional", etc.
    Expected Result: All 9 category names appear in generated prompt
    Evidence: .sisyphus/evidence/task-5-dynamic-prompt.txt

  Scenario: Prompt format rules generated per prompt_format field
    Tool: Bash (cargo test)
    Steps:
      1. Categories with prompt_format="preference" get PREFERENCE Format block
      2. Categories with prompt_format="work" get WORK Format block
      3. Categories with prompt_format=None get no special format
    Expected Result: Format sections generated correctly per category config
    Evidence: .sisyphus/evidence/task-5-format-rules.txt
  ```

  **Commit**: YES (groups with Wave 2)
  - Message: `feat(ingest): dynamic prompt generation from category registry`
  - Files: `src/ingest/prompts.rs, src/ingest/pipeline.rs`
  - Pre-commit: `cargo check`

- [x] 6. Update extractor.rs normalize_category to use registry

  **What to do**:
  - Replace `normalize_category()` (L192-199) with registry-based lookup:
    ```rust
    fn normalize_category(registry: &CategoryRegistry, tenant_id: &str, raw: &str) -> Option<String> {
        registry.normalize(tenant_id, raw)
    }
    ```
  - Update `FactExtractor::extract()` to accept and use registry
  - Pass registry through pipeline.rs to extractor

  **Must NOT do**:
  - Do NOT change LLM extraction quality or confidence thresholds

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Small function replacement, 1 file + caller update
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 7, 8)
  - **Blocks**: Task 11
  - **Blocked By**: Task 3

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/extractor.rs` L192-199 — Current normalize_category with known_variants()
  - `omem-server/src/ingest/pipeline.rs` — Caller that creates FactExtractor

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: New categories from DB are recognized by normalize_category
    Tool: Bash (cargo test)
    Steps:
      1. Seed tenant with 9 categories + aliases
      2. normalize("tenant-1", "identity") → Some("identity")
      3. normalize("tenant-1", "IDENTITY") → Some("identity") (case insensitive)
      4. normalize("tenant-1", "totally_unknown") → None
    Expected Result: Known categories resolve, unknown returns None
    Evidence: .sisyphus/evidence/task-6-normalize.txt
  ```

  **Commit**: YES (groups with Wave 2)
  - Message: `refactor(ingest): normalize_category uses CategoryRegistry`
  - Files: `src/ingest/extractor.rs, src/ingest/pipeline.rs`

- [x] 7. Update admission.rs + reconciler.rs weights from registry

  **What to do**:
  - Replace `category_prior()` in `admission.rs` (L278-288):
    ```rust
    fn category_prior(registry: &CategoryRegistry, tenant_id: &str, cat: &Category) -> f32 {
        registry.get_prior(tenant_id, cat.as_str()).unwrap_or(0.50)
    }
    ```
  - Replace `category_importance()` in `reconciler.rs` (L1087-1099):
    ```rust
    fn category_importance(registry: &CategoryRegistry, tenant_id: &str, category: &Category, quality_score: f32) -> f32 {
        let base = registry.get_importance(tenant_id, category.as_str()).unwrap_or(0.50);
        let blended = base * 0.6 + quality_score * 0.4;
        blended.clamp(0.1, 1.0)
    }
    ```
  - Update `AdmissionControl::new()` to accept `Arc<CategoryRegistry>` + `tenant_id`
  - Update `Reconciler::new()` similarly
  - Update callers in pipeline.rs

  **Must NOT do**:
  - Do NOT change the blending formula (base * 0.6 + quality * 0.4)
  - Do NOT change admission thresholds (Balanced: 0.50/0.65, Conservative: 0.58/0.72)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Two small function replacements with same logic, different data source
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 8)
  - **Blocks**: Task 11
  - **Blocked By**: Task 3

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/admission.rs` L278-288 — Current category_prior with hardcoded weights
  - `omem-server/src/ingest/reconciler.rs` L1087-1099 — Current category_importance with hardcoded weights

  **WHY Each Reference Matters**:
  - These are the two weight functions that must read from DB instead of match arms

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: category_prior reads from registry
    Tool: Bash (cargo test)
    Steps:
      1. Seed tenant, create registry
      2. category_prior(registry, "t1", &Category::new("identity")) → should return 0.75 (seed value)
      3. category_prior(registry, "t1", &Category::new("unknown")) → should return 0.50 (default)
    Expected Result: Known categories return DB weights, unknown returns default
    Evidence: .sisyphus/evidence/task-7-prior-weights.txt
  ```

  **Commit**: YES (groups with Wave 2)
  - Message: `refactor(ingest): category weights from CategoryRegistry`
  - Files: `src/ingest/admission.rs, src/ingest/reconciler.rs, src/ingest/pipeline.rs`

- [x] 8. CRUD API handler + routes for categories

  **What to do**:
  - Create `omem-server/src/api/handlers/categories.rs`:
    - `list_categories` — GET /v1/categories → returns tenant's categories
    - `get_category` — GET /v1/categories/{name} → single category
    - `create_category` — POST /v1/categories → creates new, invalidates cache
    - `update_category` — PUT /v1/categories/{name} → updates, invalidates cache
    - `delete_category` — DELETE /v1/categories/{name} → deactivates or deletes, invalidates cache
    - `list_aliases` — GET /v1/categories/aliases → returns alias mappings
    - `create_alias` — POST /v1/categories/aliases → creates alias
    - `delete_alias` — DELETE /v1/categories/aliases/{alias} → removes alias
  - DTOs:
    ```rust
    #[derive(Serialize, Deserialize)]
    pub struct CategoryResponse { ... }  // mirrors CategoryConfig
    #[derive(Serialize, Deserialize)]
    pub struct CreateCategoryBody {
        pub name: String,
        pub display_name: String,
        pub description: String,
        pub decision_rule: Option<String>,
        // ... optional fields with defaults
    }
    #[derive(Serialize, Deserialize)]
    pub struct UpdateCategoryBody { ... }  // partial update
    #[derive(Serialize, Deserialize)]
    pub struct AliasBody {
        pub alias: String,
        pub target: String,
    }
    ```
  - Add routes to `build_router()` in `api/router.rs` or `api/server.rs`:
    ```rust
    let categories_routes = Router::new()
        .route("/", get(list_categories).post(create_category))
        .route("/{name}", get(get_category).put(update_category).delete(delete_category))
        .route("/aliases", get(list_aliases).post(create_alias))
        .route("/aliases/{alias}", delete(delete_alias));
    ```
  - Register handler module in `api/handlers/mod.rs`
  - Use auth middleware (same as other /v1/ routes)

  **Must NOT do**:
  - Do NOT allow creating categories without tenant_id (from auth middleware)
  - Do NOT expose internal SQLite errors to API consumers
  - Do NOT add pagination (not needed for category lists, typically <20 items)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: New handler file + DTOs + routes, follows existing patterns but substantial code
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (but needs Tasks 1-4 complete)
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 7)
  - **Blocks**: Tasks 9, 11
  - **Blocked By**: Tasks 1, 2, 3, 4

  **References**:

  **Pattern References**:
  - `omem-server/src/api/handlers/spaces.rs` — CRUD handler pattern (create/list/get/update/delete)
  - `omem-server/src/api/handlers/clusters.rs` — Another CRUD example
  - `omem-server/src/api/server.rs` — Route registration in build_router()

  **API/Type References**:
  - `omem-server/src/api/error.rs` — OmemError → HTTP status mapping
  - `omem-server/src/domain/category.rs:CategoryConfig` — Response DTO mirrors this
  - `omem-server/src/domain/error.rs:OmemError` — Error type to use

  **WHY Each Reference Matters**:
  - spaces.rs/clusters.rs: Canonical CRUD handler patterns to follow
  - error.rs: HTTP status mapping convention
  - CategoryConfig: API response shape

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: List categories returns 9 seed categories
    Tool: Bash (curl)
    Preconditions: Server running with fresh tenant
    Steps:
      1. curl -H "X-API-Key: test-key" http://localhost:8080/v1/categories
    Expected Result: 200 OK, JSON array with 9 categories
    Failure Indicators: 404, 500, wrong count
    Evidence: .sisyphus/evidence/task-8-list-categories.json

  Scenario: Create a new category
    Tool: Bash (curl)
    Steps:
      1. curl -X POST -H "X-API-Key: test-key" -H "Content-Type: application/json" \
         -d '{"name":"custom_cat","display_name":"Custom","description":"My custom category"}' \
         http://localhost:8080/v1/categories
    Expected Result: 201 Created, category appears in subsequent GET
    Evidence: .sisyphus/evidence/task-8-create-category.json

  Scenario: Update category weight
    Tool: Bash (curl)
    Steps:
      1. curl -X PUT -H "X-API-Key: test-key" -H "Content-Type: application/json" \
         -d '{"admission_weight": 0.95}' \
         http://localhost:8080/v1/categories/identity
    Expected Result: 200 OK, weight updated
    Evidence: .sisyphus/evidence/task-8-update-category.json

  Scenario: Delete category
    Tool: Bash (curl)
    Steps:
      1. curl -X DELETE -H "X-API-Key: test-key" http://localhost:8080/v1/categories/mistakes
    Expected Result: 200 OK, category removed from list
    Evidence: .sisyphus/evidence/task-8-delete-category.json
  ```

  **Commit**: YES (groups with Wave 2)
  - Message: `feat(api): CRUD endpoints for categories management`
  - Files: `src/api/handlers/categories.rs, src/api/handlers/mod.rs, src/api/server.rs`
  - Pre-commit: `cargo check`

- [x] 9. Update stats.rs get_config endpoint

  **What to do**:
  - Replace hardcoded `"categories": ["profile","preferences","entities","events","cases","patterns"]` at `stats.rs:246` with dynamic list from CategoryRegistry
  - The handler needs access to `category_registry` via `State(state): State<Arc<AppState>>`
  - Return list of category names from `registry.get_active_categories(&tenant_id)`

  **Must NOT do**:
  - Do NOT change other stats handler behavior
  - Do NOT modify aggregation logic (already uses string-based grouping)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single line replacement in one handler
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 10, 11)
  - **Blocks**: Task 11
  - **Blocked By**: Task 8

  **References**:
  - `omem-server/src/api/handlers/stats.rs` L246 — Hardcoded categories array

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: get_config returns DB categories
    Tool: Bash (curl)
    Steps:
      1. curl -H "X-API-Key: test-key" http://localhost:8080/v1/stats/config
    Expected Result: "categories" field contains 9 seed category names
    Evidence: .sisyphus/evidence/task-9-stats-config.json
  ```

  **Commit**: YES (groups with Wave 3)
  - Message: `refactor(stats): categories list from registry`
  - Files: `src/api/handlers/stats.rs`

- [x] 10. Update memory.rs category alias normalization

  **What to do**:
  - Replace hardcoded alias map in `api/handlers/memory.rs` (L1514-1527) with CategoryRegistry aliases
  - Current code maps: experience/activities→events, knowledge/skill/skills/ability/abilities→patterns
  - These mappings should now come from `category_aliases` table via registry
  - Update the session ingest path to use `registry.normalize()` instead of manual matching

  **Must NOT do**:
  - Do NOT change Memory struct or serialization
  - Do NOT change how category is stored in LanceDB

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Replace one hardcoded mapping block with registry call
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 11)
  - **Blocks**: Task 11
  - **Blocked By**: Task 3

  **References**:
  - `omem-server/src/api/handlers/memory.rs` L1514-1527 — Hardcoded alias normalization

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Alias normalization uses DB aliases
    Tool: Bash (cargo test)
    Steps:
      1. Configure alias "experience" → "events" in DB
      2. Submit memory with category "experience"
      3. Verify stored memory has category "events"
    Expected Result: Alias resolved to target category
    Evidence: .sisyphus/evidence/task-10-alias-normalize.txt
  ```

  **Commit**: YES (groups with Wave 3)
  - Message: `refactor(api): category alias normalization from registry`
  - Files: `src/api/handlers/memory.rs`

- [x] 11. Tests — SQLite store + CategoryRegistry + API integration

  **What to do**:
  - Add inline tests in `store/sqlite.rs`:
    - `test_sqlite_store_creates_db`
    - `test_init_tables_creates_categories`
    - `test_seed_categories_inserts_9`
    - `test_seed_is_idempotent`
    - `test_crud_create_and_read`
    - `test_crud_update_changes_fields`
    - `test_crud_delete_removes_category`
    - `test_aliases_crud`
    - `test_tenant_isolation` — verify tenant A can't see tenant B's categories
  - Add inline tests in `domain/category.rs`:
    - `test_registry_loads_categories`
    - `test_registry_normalize_with_aliases`
    - `test_registry_cache_invalidation`
    - `test_registry_weights_match_seed`
    - `test_registry_unknown_category_default_weight`
  - Add inline API tests in `api/handlers/categories.rs`:
    - Use tower oneshot pattern (same as existing API tests)
    - Test GET/POST/PUT/DELETE with mock services
  - Use in-memory SQLite (`:memory:`) for all tests to avoid file cleanup

  **Must NOT do**:
  - Do NOT modify production code in this task (only add tests)
  - Do NOT use `mockall` — follow existing pattern of manual mocks

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Comprehensive test suite across 3 files, integration testing with tower oneshot
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (needs Tasks 5-10 complete for stable codebase)
  - **Parallel Group**: Wave 3 (sequential after 9, 10)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 5, 6, 7, 8, 9, 10

  **References**:

  **Pattern References**:
  - `omem-server/src/api/mod.rs` — `setup_app()` factory with TempDir, tower oneshot pattern
  - `omem-server/src/store/lancedb.rs` — Test section with MockEmbed, TrackingLlm
  - `omem-server/src/ingest/admission.rs` — Test section with `#[tokio::test]` pattern

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: All new tests pass
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server store::sqlite
      2. cargo test -p omem-server domain::category
      3. cargo test -p omem-server api::handlers::categories
    Expected Result: 0 failures across all new tests
    Evidence: .sisyphus/evidence/task-11-all-tests.txt
  ```

  **Commit**: YES (groups with Wave 3)
  - Message: `test(categories): SQLite store, CategoryRegistry, and API tests`
  - Files: `src/store/sqlite.rs, src/domain/category.rs, src/api/handlers/categories.rs`
  - Pre-commit: `cargo test`

---

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE.

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read plan end-to-end. Verify all "Must Have" implemented. Search for forbidden patterns.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check` + `cargo clippy` + `cargo test`. Review all changed files.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [x] F3. **Real Manual QA** — `unspecified-high`
  Start server, test CRUD endpoints with curl, verify seed data, verify prompt generation.
  Output: `Scenarios [N/N pass] | Integration [N/N] | VERDICT`

- [x] F4. **Scope Fidelity Check** — `deep`
  Verify no scope creep: no scoring_weights, no Web UI, no LanceDB changes.
  Output: `Tasks [N/N compliant] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `feat(store): add SqliteStore with categories tables` — Cargo.toml, store/sqlite.rs, store/sqlite_schema.rs, domain/category.rs, api/server.rs, main.rs
- **Wave 2**: `feat(categories): dynamic prompts + weights from DB` — prompts.rs, extractor.rs, admission.rs, reconciler.rs, api/handlers/categories.rs
- **Wave 3**: `feat(categories): CRUD API + tests` — stats.rs, memory.rs, tests
- Pre-commit: `cargo check && cargo test`

---

## Success Criteria

### Verification Commands
```bash
cargo check                              # Expected: Finished dev profile
cargo test                               # Expected: 370+ passed, 0 new failures
cargo test -p omem-server store::sqlite  # Expected: all SQLite tests pass
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass (no new failures)
- [ ] CRUD API functional (curl verified)
- [ ] Dynamic prompts reflect DB categories
