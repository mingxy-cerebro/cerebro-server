# omem-server/src/ — Rust Core Source

Cerebro server core: axum HTTP layer, 11-stage ingestion, hybrid retrieval, lifecycle management, memory clustering, and vector storage.

## Overview

This directory contains the entire Rust backend for Cerebro — a shared persistent memory system for AI agents. Built on axum 0.8 + LanceDB 0.27, it provides 48+ REST endpoints, vector search with hybrid retrieval, automatic memory lifecycle management, and multi-tenant space-based sharing.

| Metric | Value |
|--------|-------|
| Source files | 92 |
| Lines of code | ~28,927 |
| Top-level modules | 13 |
| Inline tests | 373 across 49 files |
| Crate | `omem-server` |
| Framework | axum 0.8, tokio, tower-http |
| Vector DB | LanceDB 0.27, arrow 57 |

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                              main.rs                                 │
│  rustls → Config → Tracing → Stores → Embed → LLMs → ClusterStore   │
│                              ↓                                       │
│                         AppState (15 fields)                         │
│                              ↓                                       │
│                       build_router(state)                            │
│                              ↓                                       │
│              ┌───────────────┼───────────────┐                       │
│              │               │               │                       │
│              ▼               ▼               ▼                       │
│      ┌───────────┐   ┌───────────┐   ┌───────────┐                  │
│      │  Public   │   │  Authed   │   │ Lifecycle │                  │
│      │  routes   │   │  routes   │   │ Scheduler │                  │
│      │ /health   │   │ /v1/...   │   │ (spawned) │                  │
│      └─────┬─────┘   └─────┬─────┘   └───────────┘                  │
│            │               │                                        │
│            └───────┬───────┘                                        │
│                    ▼                                                │
│              Middleware stack                                       │
│       CORS → logging → auth (from_fn_with_state)                    │
│                    ▼                                                │
│              ┌───────────────┐                                      │
│              │   Handlers    │                                      │
│              │ (12 files)    │                                      │
│              └───────┬───────┘                                      │
│                      │                                              │
│      ┌───────────────┼───────────────┬───────────────┐              │
│      ▼               ▼               ▼               ▼              │
│ ┌─────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐           │
│ │ ingest/ │   │ retrieve/│   │lifecycle/│   │ cluster/ │           │
│ │Pipeline │   │ Pipeline │   │Scheduler │   │ Manager  │           │
│ └────┬────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘           │
│      │             │              │              │                  │
│      ▼             ▼              ▼              ▼                  │
│ ┌─────────────────────────────────────────────────────┐             │
│ │                   store/ + embed/ + llm/             │             │
│ │  StoreManager → LanceStore (per-tenant, LRU cache)  │             │
│ │  EmbedService → openai-compat / bedrock / noop      │             │
│ │  LlmService   → openai-compat / bedrock / noop      │             │
│ └─────────────────────────────────────────────────────┘             │
│                              │                                      │
│                              ▼                                      │
│                         domain/ (models)                            │
│                    Memory, Space, Tenant, Profile                   │
└─────────────────────────────────────────────────────────────────────┘
```

## Module Map

| Module | Files | Key Types / Traits | Purpose |
|--------|-------|-------------------|---------|
| `api/` | 25 | `AppState`, `build_router`, `OmemError` impl | HTTP layer: routing, middleware, handlers, event bus |
| `cluster/` | 7 | `ClusterManager`, `ClusterStore`, `ClusterAssigner` | Memory clustering: k-means, auto-assignment, aggregation |
| `config/` | 1 | `OmemConfig` | Environment-based configuration (OMEM_* prefix) |
| `connectors/` | 2 | `GitHubConnector` | External integrations: GitHub webhooks |
| `domain/` | 10 | `Memory`, `Space`, `Tenant`, `OmemError` | Core domain models and error enum |
| `embed/` | 5 | `EmbedService` trait | Embedding provider abstraction + implementations |
| `ingest/` | 12 | `IngestPipeline`, `Reconciler`, `FactExtractor` | 11-stage ingestion: extract, reconcile, admit, noise filter |
| `lifecycle/` | 5 | `LifecycleScheduler`, `DecayEngine`, `TierManager` | Weibull decay, auto-forgetting, tier promotion/demotion |
| `llm/` | 5 | `LlmService` trait, `complete_json` | LLM provider abstraction + JSON repair helpers |
| `multimodal/` | 6 | `MultiModalService`, `ContentType` | PDF, image, video, code AST processing |
| `profile/` | 2 | `ProfileService` | Auto-generated user profiles from accumulated memories |
| `retrieve/` | 4 | `RetrievalPipeline`, `Reranker`, `RetrievalTrace` | 11-stage hybrid search: vector + BM25 + RRF + rerank |
| `store/` | 5 | `StoreManager`, `LanceStore`, `TenantStore`, `SpaceStore` | LanceDB vector CRUD, tenant/session caching |

### Handler Files (`api/handlers/`)

| File | Endpoints |
|------|-----------|
| `memory.rs` | CRUD, search, batch operations, merge, re-embed, optimize |
| `sharing.rs` | Share, pull, unshare, reshare, batch-share, share-to-user, org setup/publish |
| `spaces.rs` | Create, list, get, update, delete spaces; member management |
| `imports.rs` | File imports, intelligence, rollback, cross-reconcile |
| `files.rs` | Multipart file upload (PDF, image, video, code) |
| `stats.rs` | Tags, decay curves, relations, config, agents, sharing stats |
| `profile.rs` | User profile retrieval |
| `tenant.rs` | Tenant creation and lookup |
| `session_recalls.rs` | Session-based recall tracking |
| `vault.rs` | Vault password management |
| `github.rs` | GitHub connector webhook handler |
| `clusters.rs` | Cluster CRUD, trigger clustering, jobs, stats |
| `scheduler.rs` | Scheduler status, pause/resume lifecycle & clustering |
| `lifecycle.rs` | Manual lifecycle trigger |
| `events.rs` | SSE event stream |
| `mod.rs` + `merge.rs` | Handler exports, memory merge helper |

## Key Types & Traits

### `AppState` (`api/server.rs`)

Central application state (15 fields). Passed to all handlers via axum `State` extractor:

```rust
pub struct AppState {
    pub store_manager: Arc<StoreManager>,      // Per-tenant LanceDB LRU cache
    pub tenant_store: Arc<TenantStore>,        // Tenant metadata persistence
    pub space_store: Arc<SpaceStore>,          // Space membership & metadata
    pub embed: Arc<dyn EmbedService>,          // Embedding provider
    pub llm: Arc<dyn LlmService>,              // Primary LLM
    pub recall_llm: Arc<dyn LlmService>,       // Optional separate recall LLM
    pub cluster_llm: Arc<dyn LlmService>,      // Optional cheaper cluster/profile LLM
    pub cluster_store: Arc<ClusterStore>,      // Cluster metadata persistence
    pub config: OmemConfig,                    // Runtime configuration
    pub import_semaphore: Arc<Semaphore>,      // Concurrency limit: 3 imports
    pub reconcile_semaphore: Arc<Semaphore>,   // Concurrency limit: 1 reconcile
    pub event_bus: SharedEventBus,             // SSE event publisher
    pub scheduler_control: SharedSchedulerControl, // Pause/resume scheduler
    pub session_locks: Arc<DashMap<String, Arc<Mutex<()>>>>, // Session dedup locks
    pub reranker: Option<Reranker>,            // Optional cross-encoder reranker
}
```

### `EmbedService` (`embed/service.rs`)

```rust
#[async_trait::async_trait]
pub trait EmbedService: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, OmemError>;
    fn dimensions(&self) -> usize;
}
```

Implementations: `openai_compat.rs`, `bedrock.rs` (feature-gated), `noop.rs` (testing).

### `LlmService` (`llm/service.rs`)

```rust
#[async_trait::async_trait]
pub trait LlmService: Send + Sync {
    async fn complete_text(&self, system: &str, user: &str) -> Result<String, OmemError>;
}
```

Helper: `complete_json<T>()` — parses LLM response as typed JSON with automatic repair and one retry. Also strips `<think>` tags and markdown fences.

Implementations: `openai_compat.rs`, `bedrock.rs` (feature-gated), `noop.rs` (testing).

### `StoreManager` (`store/manager.rs`)

```rust
pub struct StoreManager {
    base_uri: String,
    cache: Mutex<HashMap<String, CacheEntry>>,        // LRU, max 20
    session_cache: Mutex<HashMap<String, SessionCacheEntry>>,
    max_cached: usize,
}
```

- `get_store(tenant_id) -> Result<Arc<LanceStore>, OmemError>` — most common call (repeated ~42x)
- `get_accessible_stores(tenant_id, spaces) -> Vec<AccessibleStore>` — cross-space search
- LRU eviction when cache exceeds 20 entries

### `TenantStore` (`store/tenant.rs`)

Persists tenant records (id, name, api_key, created_at) in a system-wide LanceDB table.

### `SpaceStore` (`store/spaces.rs`)

Persists space metadata and member roles. Handles space creation, member add/remove, role updates.

### `ClusterStore` (`cluster/cluster_store.rs`)

Persists cluster metadata (centroids, labels, member counts) in LanceDB. Used by `ClusterManager` and `ClusterAssigner`.

## Startup Sequence

Exact order from `main.rs`:

1. **`rustls::crypto::ring::default_provider().install_default()`** — TLS crypto provider
2. **`OmemConfig::from_env()`** — load all configuration from environment variables
3. **`init_tracing(&config)`** — JSON-formatted tracing subscriber with `EnvFilter`
4. **`StoreManager::new(&base_uri)`** — create per-tenant LanceDB connection manager
5. **`TenantStore::new(&system_uri)`** + **`init_table()`** — tenant metadata persistence
6. **`SpaceStore::new(&system_uri)`** + **`init_tables()`** — space membership persistence
7. **`create_embed_service(&config)`** — embedding provider (noop/openai/bedrock)
8. **`create_llm_service(&config)`** — primary LLM for extraction/reconciliation
9. **`create_recall_llm_service(&config)`** — separate recall LLM (falls back to primary if unconfigured)
10. **`create_cluster_llm_service(&config)`** — cheaper cluster/profile LLM (falls back to primary)
11. **`ClusterStore::new(...)`** — cluster metadata persistence
12. **`AppState { ... }`** — assemble all services into shared state
13. **`build_router(state.clone())`** — construct axum router with all routes and middleware
14. **`optimize_all_on_disk()`** — background LanceDB cleanup (spawned task)
15. **`LifecycleScheduler::new(...)`** + **`with_event_bus`** + **`with_scheduler_control`**
16. **`tokio::spawn(lifecycle_scheduler.run())`** — start periodic decay/forgetting/clustering
17. **`TcpListener::bind(&addr)`** — bind to configured port
18. **`axum::serve(listener, app).with_graceful_shutdown(shutdown_signal())`** — serve with Ctrl-C / SIGTERM graceful shutdown

Concurrency limits:
- Import semaphore: 3 concurrent imports
- Reconcile semaphore: 1 concurrent reconciliation

## Conventions & Patterns

### Error Handling
- **Single enum**: `OmemError` in `domain/error.rs` using `thiserror`
- **No `anyhow`**: All errors are typed. No `Result<T>` alias.
- **HTTP mapping**: `api/error.rs` implements `IntoResponse` mapping variants to status codes:
  - `NotFound` → 404, `Unauthorized` → 401, `Validation` → 400, `RateLimited` → 429, `Internal/Storage/Embedding/Llm` → 500

### Handler Signatures
```rust
async fn handler_name(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<String>,
    Path(id): Path<String>,
    Query(params): Query<SomeDto>,
    Json(body): Json<SomeBody>,
) -> Result<impl IntoResponse, OmemError>
```

### Store Access Pattern
```rust
let store = state.store_manager.get_store(&tenant_id).await?;
// ~42 occurrences across handlers
```

### Naming Conventions
- Handlers: `snake_case` (e.g., `search_memories`, `create_space`)
- DTOs: `PascalCase` with suffix: `*Body`, `*Response`, `*Dto`
- Modules: plural for collections (`memories`, `spaces`, `stats`)

### Configuration
- `OmemConfig` implements `Default` + `from_env()`
- All env vars prefixed with `OMEM_`
- Storage resolution: `OSS > S3 > local ./omem-data`

### Serialization
- `serde(rename_all = "snake_case")` on request/response DTOs
- `serde(default)` for optional fields
- `skip_serializing_if = "Option::is_none"` for optional response fields

### Middleware Stack
Applied in `build_router()` in order:
1. **CORS** — `CorsLayer::new().allow_origin(Any)` (global)
2. **Logging** — `logging_middleware` via `from_fn` (global)
3. **Auth** — `auth_middleware` via `from_fn_with_state(state, ...)` (authed routes only)

Auth validates `X-API-Key` header against `TenantStore`, injects `Extension(tenant_id)`.

### Space ID Normalization
```rust
pub fn normalize_space_id(space_id: &str) -> String {
    // "team:abc" → "team/abc"
    // "personal:xyz" → "personal/xyz"
}
```
Legacy colon format is auto-converted to slash format.

## Testing

- **373 inline tests** across **49 files** with `#[cfg(test)]`
- **100% inline** — no separate `tests/` directory
- **Manual mock traits** — no `mockall`. Test embedders/LLMs implement traits directly:
  ```rust
  struct TestEmbedder;
  #[async_trait::async_trait]
  impl EmbedService for TestEmbedder { ... }
  ```
- **TempDir isolation** — integration tests in `api/mod.rs` use `tempfile::TempDir` for LanceDB storage
- **Factory functions** — `setup_app()` constructs full router with mock services
- **Tower oneshot** — integration tests use `tower::ServiceExt::oneshot` for HTTP testing without binding a port

### Key Test Patterns
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn unit_test() { ... }

    #[tokio::test]
    async fn integration_test() {
        let (app, _dir) = setup_app().await;
        let response = app.oneshot(Request::builder()...).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
```

## Build Commands

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run server locally
cargo run

# Run all inline tests
cargo test

# Run tests for specific package
cargo test -p omem-server

# Lint
cargo clippy

# Build without AWS Bedrock (for musl static linking)
cargo build --no-default-features
```

### musl Static Build
```bash
rustup target add x86_64-unknown-linux-musl
RUSTFLAGS="-C target-feature=+crt-static -C relocation-model=static" \
  cargo build --release --target x86_64-unknown-linux-musl \
  -p omem-server --no-default-features
```
> musl build excludes Bedrock due to `aws-lc-sys` incompatibility. Use OpenAI-compatible providers (e.g., DashScope, SiliconFlow) instead.

## Important Warnings

1. **`expect()` in main.rs**: 13 occurrences — all are startup-only (store init, LLM creation, TCP bind). Acceptable for startup fatal errors, but each should have a clear error message.

2. **`unwrap/expect` in production code**: ~28 occurrences outside `#[cfg(test)]` blocks across 45 files. These need periodic audit — most are in initialization paths or cache lookups, but any in request handlers are bugs waiting to happen.

3. **`AppState` God Object**: 15 fields. Consider grouping related fields into sub-structs (e.g., `StoreServices`, `LlmServices`, `ConcurrencyLimits`) to reduce handler dependencies and improve testability.

4. **No clippy/rustfmt config**: No `.clippy.toml` or `rustfmt.toml` present. Consider adding project-specific lint rules and formatting standards.

5. **`cluster/` module is undocumented in root AGENTS.md**: This is a new module (not in the top-level project overview). It adds memory clustering with k-means, LLM-based cluster assignment, and background clustering tasks. Any agent working on clustering should read `cluster/mod.rs` and the cluster-specific files first.

## Hierarchical AGENTS.md

This project uses hierarchical knowledge bases:

| Location | Scope |
|----------|-------|
| `../../AGENTS.md` | Project overview, deployment, plugins |
| `AGENTS.md` (this file) | Rust core source: modules, types, conventions |
| `api/AGENTS.md` | HTTP layer: routes, handlers, middleware, AppState details |
| `ingest/AGENTS.md` | 11-stage ingestion pipeline: stages, LLM prompts, decisions |
