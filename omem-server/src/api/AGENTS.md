# API Layer

The HTTP interface for Cerebro — axum 0.8 router, middleware stack, handlers, and shared state.
**26 Rust source files, ~10,000 lines, 55+ unique REST endpoints.**

---

## Overview

The `api` module is the entry point for all external traffic. It:

- Defines the axum `Router` with route tables for 55+ endpoints
- Provides `AppState` — the 15-field shared state injected into every handler
- Runs a middleware stack: CORS → request logging → API-key auth
- Maps domain `OmemError` variants to HTTP status codes
- Publishes `ServerEvent`s via `EventBus` for SSE streaming
- Exposes scheduler pause/resume controls via `SchedulerControl`

---

## Structure

```
api/
├── mod.rs                    (1,664 lines) — module declarations + 35 integration tests
├── router.rs                 (161 lines) — route table definition
├── server.rs                 (52 lines) — AppState struct + helpers
├── error.rs                  (79 lines) — IntoResponse impl for OmemError
├── event_bus.rs              (45 lines) — broadcast-based event bus
├── scheduler_control.rs      (59 lines) — lifecycle/clustering pause flags
├── middleware/
│   ├── mod.rs                (5 lines)
│   ├── auth.rs               (72 lines) — X-API-Key validation
│   └── logging.rs            (35 lines) — request_id + duration tracing
└── handlers/
    ├── mod.rs                (59 lines) — re-exports
    ├── memory.rs             (1,853 lines) — memory CRUD, search, batch ops
    ├── sharing.rs            (2,072 lines) — share, pull, unshare, org mgmt
    ├── stats.rs              (1,230 lines) — analytics, tags, decay, relations
    ├── session_recalls.rs    (530 lines) — recall decisions + session state
    ├── imports.rs            (448 lines) — file import, intelligence, rollback
    ├── clusters.rs           (404 lines) — cluster CRUD, jobs, triggers
    ├── spaces.rs             (343 lines) — space CRUD + membership
    ├── merge.rs              (169 lines) — memory merge
    ├── lifecycle.rs           (173 lines) — manual lifecycle trigger
    ├── files.rs              (124 lines) — multimodal file upload
    ├── vault.rs              (122 lines) — vault password management
    ├── github.rs             (94 lines) — GitHub connector + webhook
    ├── tenant.rs             (97 lines) — tenant creation / lookup
    ├── profile.rs            (52 lines) — user profile retrieval
    ├── scheduler.rs          (49 lines) — scheduler status & pause/resume
    └── events.rs             (35 lines) — SSE event stream
```

---

## AppState

Defined in `server.rs`. Passed as `State<Arc<AppState>>` to every handler.

| Field | Type | Purpose |
|-------|------|---------|
| `store_manager` | `Arc<StoreManager>` | Creates per-space LanceDB connections |
| `tenant_store` | `Arc<TenantStore>` | Tenant metadata persistence |
| `space_store` | `Arc<SpaceStore>` | Space membership & metadata |
| `embed` | `Arc<dyn EmbedService>` | Primary embedding service |
| `llm` | `Arc<dyn LlmService>` | Primary LLM (extraction, completion) |
| `recall_llm` | `Arc<dyn LlmService>` | Separate LLM for recall decisions |
| `cluster_llm` | `Arc<dyn LlmService>` | LLM for cluster summarization |
| `cluster_store` | `Arc<ClusterStore>` | Cluster metadata persistence |
| `config` | `OmemConfig` | Runtime configuration (env vars) |
| `import_semaphore` | `Arc<Semaphore>(3)` | Concurrency limit for imports |
| `reconcile_semaphore` | `Arc<Semaphore>(1)` | Serializes reconciliation |
| `event_bus` | `SharedEventBus` | Broadcast channel for SSE events |
| `scheduler_control` | `SharedSchedulerControl` | Pause/resume flags for background tasks |
| `session_locks` | `Arc<DashMap<String, Arc<Mutex<()>>>>` | Per-session ingestion locks |
| `reranker` | `Option<Reranker>` | Cross-encoder reranker (optional) |

### Helpers

- `personal_space_id(tenant_id: &str) -> String` — formats `personal/{tenant_id}`
- `normalize_space_id(space_id: &str) -> String` — converts legacy `team:abc` → `team/abc`

---

## Route Map

Grouped by handler file. Public routes (no auth) are marked with **(public)**.

### memory.rs
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/v1/memories/search` | `search_memories` |
| POST   | `/v1/memories/batch-delete` | `batch_delete` |
| POST   | `/v1/memories/batch-get` | `batch_get_memories` |
| POST   | `/v1/memories/merge` | `merge_memories` |
| POST   | `/v1/memories/batch-visibility` | `batch_update_visibility` |
| DELETE | `/v1/memories/all` | `delete_all_memories` |
| GET    | `/v1/memories/{id}` | `get_memory` |
| PUT    | `/v1/memories/{id}` | `update_memory` |
| DELETE | `/v1/memories/{id}` | `delete_memory` |
| GET    | `/v1/memories` | `list_memories` |
| POST   | `/v1/memories` | `create_memory` |
| POST   | `/v1/memories/session-ingest` | `session_ingest` |
| POST   | `/v1/memories/re-embed` | `reembed_memories` |
| POST   | `/v1/memories/optimize` | `optimize_memories` |

### profile.rs
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/v1/profile` | `get_profile` |

### stats.rs
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/v1/stats` | `get_stats` |
| GET    | `/v1/stats/config` | `get_config` |
| GET    | `/v1/stats/tags` | `get_tags` |
| GET    | `/v1/stats/decay` | `get_decay` |
| GET    | `/v1/stats/relations` | `get_relations` |
| GET    | `/v1/stats/spaces` | `get_spaces_stats` |
| GET    | `/v1/stats/sharing` | `get_sharing_stats` |
| GET    | `/v1/stats/agents` | `get_agents_stats` |

### merge.rs / lifecycle.rs
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/memories/merge` | `merge_memories` |
| GET    | `/v1/tier-changes` | `get_tier_changes` |
| POST   | `/v1/tier-changes/delete` | `delete_tier_history_entry` |
| POST   | `/v1/lifecycle/trigger` | `trigger_lifecycle` |

### files.rs
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/files` | `upload_file` |

### imports.rs
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/imports` | `create_import` |
| GET    | `/v1/imports` | `list_imports` |
| GET    | `/v1/imports/{id}` | `get_import` |
| POST   | `/v1/imports/{id}/intelligence` | `trigger_intelligence` |
| POST   | `/v1/imports/{id}/rollback` | `rollback_import` |
| POST   | `/v1/imports/cross-reconcile` | `cross_reconcile` |

### github.rs
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/connectors/github/connect` | `github_connect` |
| POST   | `/v1/connectors/github/webhook` | `github_webhook` **(public)** |

### spaces.rs
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/v1/spaces` | `list_spaces` |
| POST   | `/v1/spaces` | `create_space` |
| GET    | `/v1/spaces/{id}` | `get_space` |
| PUT    | `/v1/spaces/{id}` | `update_space` |
| DELETE | `/v1/spaces/{id}` | `delete_space` |
| POST   | `/v1/spaces/{id}/members` | `add_member` |
| DELETE | `/v1/spaces/{id}/members/{user_id}` | `remove_member` |
| PUT    | `/v1/spaces/{id}/members/{user_id}` | `update_member_role` |

### sharing.rs
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/memories/{id}/share` | `share_memory` |
| POST   | `/v1/memories/{id}/pull` | `pull_memory` |
| POST   | `/v1/memories/{id}/unshare` | `unshare_memory` |
| POST   | `/v1/memories/{id}/reshare` | `reshare_memory` |
| POST   | `/v1/memories/batch-share` | `batch_share` |
| POST   | `/v1/memories/share-all` | `share_all` |
| POST   | `/v1/memories/{id}/share-to-user` | `share_to_user` |
| POST   | `/v1/memories/share-all-to-user` | `share_all_to_user` |
| POST   | `/v1/org/setup` | `org_setup` |
| POST   | `/v1/org/{id}/publish` | `org_publish` |
| GET    | `/v1/spaces/{id}/auto-share-rules` | `list_auto_share_rules` |
| POST   | `/v1/spaces/{id}/auto-share-rules` | `create_auto_share_rule` |
| DELETE | `/v1/spaces/{id}/auto-share-rules/{rule_id}` | `delete_auto_share_rule` |

### vault.rs
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/vault/password` | `set_vault_password` |
| POST   | `/v1/vault/verify` | `verify_vault_password` |
| DELETE | `/v1/vault/password` | `delete_vault_password` |
| GET    | `/v1/vault/status` | `get_vault_status` |

### session_recalls.rs
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/should-recall` | `should_recall` |
| GET    | `/v1/session-recalls` | `list_session_recalls` |
| POST   | `/v1/session-recalls` | `create_session_recall` |
| GET    | `/v1/session-recalls/{id}` | `get_session_recall` |
| DELETE | `/v1/session-recalls/{id}` | `delete_session_recall` |

### clusters.rs
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/v1/clusters` | `list_clusters` |
| POST   | `/v1/clusters/batch-delete` | `batch_delete_clusters` |
| DELETE | `/v1/clusters/all` | `delete_all_clusters` |
| POST   | `/v1/clusters/trigger` | `trigger_clustering` |
| GET    | `/v1/clusters/jobs` | `list_clustering_jobs` |
| GET    | `/v1/clusters/jobs/{id}` | `get_clustering_job` |
| DELETE | `/v1/clusters/jobs/{id}` | `delete_clustering_job` |
| GET    | `/v1/clusters/stats` | `get_clustering_stats` |
| GET    | `/v1/clusters/{id}` | `get_cluster` |
| DELETE | `/v1/clusters/{id}` | `delete_cluster` |

### scheduler.rs
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/v1/scheduler/status` | `get_scheduler_status` |
| POST   | `/v1/scheduler/lifecycle/pause` | `pause_lifecycle` |
| POST   | `/v1/scheduler/lifecycle/resume` | `resume_lifecycle` |
| POST   | `/v1/scheduler/clustering/pause` | `pause_clustering` |
| POST   | `/v1/scheduler/clustering/resume` | `resume_clustering` |

### events.rs
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/v1/events` | `sse_events` |

### tenant.rs **(public)**
| Method | Path | Handler |
|--------|------|---------|
| POST   | `/v1/tenants` | `create_tenant` |
| GET    | `/v1/tenants/{id}` | `get_tenant` |

### Health **(public)**
| Method | Path | Handler |
|--------|------|---------|
| GET    | `/health` | `health` |

---

## Handler Patterns

### Signature Pattern

```rust
pub async fn handler_name(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
    Query(q): Query<SomeQuery>,
    Json(body): Json<SomeBody>,
) -> Result<impl IntoResponse, OmemError>
```

- `State` — shared `AppState`
- `Extension(auth)` — `AuthInfo { tenant_id, agent_id }` injected by auth middleware
- `Path` / `Query` / `Json` — standard axum extractors
- Return type is always `Result<..., OmemError>` which implements `IntoResponse`

### Store Access Pattern

```rust
let space_id = personal_space_id(&auth.tenant_id);
let store = state.store_manager.get_store(&space_id).await?;
```

For cross-space operations (e.g., sharing, search), handlers normalize space IDs and iterate over accessible spaces.

### Error Handling

`OmemError` variants (from `domain::error.rs`) are mapped to HTTP status codes in `api/error.rs`:

| Variant | HTTP Status | Code in JSON |
|---------|-------------|--------------|
| `NotFound` | 404 | `not_found` |
| `Unauthorized` | 401 | `unauthorized` |
| `Validation` | 400 | `validation_error` |
| `RateLimited` | 429 | `rate_limited` |
| `Storage` | 500 | `internal_error` |
| `Embedding` | 500 | `internal_error` |
| `Llm` | 500 | `internal_error` |
| `Internal` | 500 | `internal_error` |

All errors are logged at `ERROR` level with status, code, and message.

---

## Middleware Stack

Request processing order (outer → inner):

1. **CORS** (`CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)`) — applied to all routes
2. **Logging** (`logging_middleware`) — generates `request_id` UUID, creates a tracing span, logs start + duration + status
3. **Auth** (`auth_middleware`) — applied only to `authed_routes` via `route_layer`
   - Reads `X-API-Key` header, falls back to `api_key=` query param (for EventSource)
   - Looks up tenant in `TenantStore`
   - Checks tenant status is `active`
   - Injects `AuthInfo` into request extensions
   - Returns 401 on missing/invalid key

### Public Routes (no auth)

- `GET /health`
- `POST /v1/tenants`
- `GET /v1/tenants/{id}`
- `POST /v1/connectors/github/webhook`

---

## Key Types

### Request/Response DTOs

Each handler file defines its own `Deserialize` request DTOs and `Serialize` response structs near the top of the file. Examples:

- `CreateMemoryBody` — handles both direct creation (`content`) and message-based ingest (`messages`)
- `SearchQuery` — `q`, `limit`, `scope`, `min_score`, `include_trace`, `space`, `tags`, `check_stale`
- `ShareRequest` / `PullRequest` / `UnshareRequest` — sharing operations
- `StatsResponse` / `TimelineEntry` — analytics aggregation
- `UploadResponse` — file upload confirmation

### Shared Types

- `AuthInfo` (`domain::tenant`) — `tenant_id: String, agent_id: Option<String>`
- `ServerEvent` (`api::event_bus`) — `event_type, tenant_id, data, timestamp`
- `ImportTaskRecord` (`store::spaces`) — import task persistence

---

## Handler Details

| File | Lines | Key Endpoints / Responsibility |
|------|-------|-------------------------------|
| `memory.rs` | 1,853 | Memory CRUD, hybrid search (`/search`), batch ops, session ingest, tier history, re-embed, optimize. Largest handler. |
| `sharing.rs` | 2,072 | Share/pull/unshare/reshare, batch share, share-all, auto-share rules, org setup/publish, cross-user sharing. Largest file. |
| `stats.rs` | 1,230 | Aggregated statistics, tag distribution, decay curve computation, relation graph, space/sharing/agent stats. |
| `session_recalls.rs` | 530 | `should_recall` LLM-based gate, session recall CRUD, per-session memory caching. |
| `imports.rs` | 448 | Multipart file import, SHA256 dedup, post-process trigger, rollback, cross-reconcile. |
| `clusters.rs` | 404 | Cluster CRUD, background job management, trigger clustering, stats. |
| `spaces.rs` | 343 | Space CRUD (personal/team/org), member add/remove/role update. |
| `merge.rs` | 169 | Merge two memories into one. |
| `lifecycle.rs` | 173 | Manual lifecycle trigger endpoint. |
| `files.rs` | 124 | Multimodal file upload (PDF, image, video, code). Enforces 50MB limit. |
| `vault.rs` | 122 | Vault password set/verify/delete with SHA256+salt hashing. |
| `github.rs` | 94 | GitHub connector OAuth initiation and webhook receiver. |
| `tenant.rs` | 97 | Tenant creation (public) and lookup (public). Auto-creates personal space. |
| `profile.rs` | 52 | Returns auto-generated user profile (static facts + dynamic context). |
| `scheduler.rs` | 49 | Scheduler status, pause/resume for lifecycle and clustering. |
| `events.rs` | 35 | SSE event stream (`/v1/events`) using `EventBus` broadcast channel. |

---

## Warnings

1. **File size bloat**
   `sharing.rs` (2,072 lines) and `memory.rs` (1,853 lines) are too large. Consider splitting by domain:
   - `sharing.rs` → `share_ops.rs`, `auto_share.rs`, `org.rs`
   - `memory.rs` → `memory_crud.rs`, `memory_search.rs`, `memory_batch.rs`

2. **DRY violation: `personal_space_id`**
   The helper is invoked 55 times across 11 handler files. Most handlers repeat the same `let space_id = personal_space_id(&auth.tenant_id);` line. Extracting a `current_space_id(auth: &AuthInfo)` extractor or middleware would eliminate this repetition.

3. **No multipart size limit in `imports.rs`**
   `create_import` reads the entire uploaded file into memory without checking size. An adversarial upload could cause OOM. Add a limit matching `files.rs` (50MB) or stream-process the upload.

4. **`session_locks` DashMap never cleaned up**
   `AppState.session_locks` accumulates one entry per unique `session_id` forever. Long-running servers will leak memory. Add TTL eviction or clean up on session end.

5. **`LAST_RECALL_TIME` global HashMap never cleaned**
   `session_recalls.rs` uses a `LazyLock<Arc<Mutex<HashMap<...>>>>` to track last recall timestamps per session. Entries are never removed. Add periodic cleanup or use a TTL cache.

6. **`session_recalls.rs` unbounded `Arc<Mutex<HashMap>>` growth**
   Related to #5 — the mutex-protected HashMap grows without bound as new sessions are seen.

---

## Testing

Integration tests live in `api/mod.rs` (35 tests, ~1,500 lines). They use a shared `setup_app()` helper that:

1. Creates a `tempfile::TempDir` for LanceDB storage
2. Initializes `TenantStore`, `SpaceStore`, and `ClusterStore`
3. Wires noop `TestEmbedder` (1024-dim, fixed vector) and `TestLlm` (returns `{"memories":[]}`)
4. Builds the full axum router with real `AppState`
5. Uses `tower::ServiceExt::oneshot` to send requests and assert status codes + JSON bodies

### Test Patterns

- **Auth testing**: Verify 401 on missing/invalid `X-API-Key`
- **CRUD round-trips**: Create → Get → Update → Delete
- **Search verification**: Create memory with content, search with query, assert results array
- **Multipart uploads**: Helper `build_multipart()` constructs test form-data bodies
- **CORS**: OPTIONS request asserts `access-control-allow-origin: *`

### Running Tests

```bash
cargo test -p omem-server api::tests
```

---

## Dependencies

Key crates used in this module:

- `axum` 0.8 — HTTP framework
- `tower-http` 0.6 — CORS, trace layers
- `serde` / `serde_json` — DTO serialization
- `tokio` — async runtime, `Semaphore`, `Mutex`, `Notify`
- `dashmap` — `session_locks` concurrent map
- `uuid` — request IDs and entity generation
- `chrono` — timestamps in session recall tracking
- `sha2` — SHA256 for import deduplication and vault password hashing
- `axum-extra` — `Multipart` extractor
