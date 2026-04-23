# Cerebro Server (omem-server-source)

Cerebro — Shared Persistent Memory for AI Agents. Rust HTTP server with vector search, 11-stage ingestion, and TypeScript agent plugins.

## Architecture

```
omem-server-source/
  omem-server/          # Rust backend (axum 0.8 + lancedb 0.27)
    src/
      main.rs           # Bootstrap: tracing -> OmemConfig -> stores -> services -> router -> scheduler -> serve
      config.rs         # All config via env vars (OMEM_*)
      api/              # HTTP layer: router, handlers, middleware
      ingest/           # 11-stage ingestion pipeline
      domain/           # Core domain models
      store/            # Vector storage (lancedb)
      embed/            # Embedding services
      llm/              # LLM services
      retrieve/         # Hybrid retrieval with reranking
      multimodal/       # Code/PDF/image/video processing
      lifecycle/        # Weibull decay, auto-forgetting, tier management
      profile/          # User profile auto-generation
      connectors/       # GitHub integration
  plugins/
    opencode/           # OpenCode memory plugin (TypeScript)
    openclaw/           # OpenClaw plugin (TypeScript)
    mcp/                # MCP server plugin (TypeScript)
    claude-code/        # Claude Code plugin (hooks + skills)
```

### Startup Flow

`main.rs`: tracing init -> `OmemConfig::from_env()` -> `StoreManager` -> `TenantStore` -> `SpaceStore` -> `EmbedService` -> `LlmService` (primary + recall) -> `AppState` -> `build_router()` -> `LifecycleScheduler` (spawned) -> `axum::serve` with graceful shutdown

Concurrency limits: import semaphore (3), reconcile semaphore (1)

### Core Concepts

- **Tenant isolation**: API Key = Tenant ID
- **Three-tier Space**: Personal / Team / Organization
- **License**: Apache-2.0

## Module Reference

### api/ — HTTP Layer
- `router.rs`, `server.rs`, `middleware/` — Route definitions, AppState, auth middleware
- `handlers/` — 12 handler files: files, github, imports, memory, mod, profile, session_recalls, sharing, spaces, stats, tenant, vault — 48+ REST endpoints
- `error.rs` — Unified error response types

### ingest/ — 11-Stage Ingestion Pipeline
- `pipeline.rs` — Orchestrator for the full pipeline
- `admission.rs` — Input validation and admission control
- `extractor.rs` — Fact/entity extraction from content
- `intelligence.rs` — Semantic intelligence (categorization, tagging)
- `noise.rs` — Noise filtering and deduplication
- `privacy.rs` — PII/sensitive content detection and redaction
- `prompts.rs` — LLM prompt templates for extraction
- `reconciler.rs` — Memory reconciliation (merge, update, deduplicate)
- `session.rs` — Session-bound ingestion context
- `preference_slots.rs` — Preference extraction and slot filling
- `types.rs` — Shared ingestion types

### domain/ — Core Models
- `memory.rs`, `category.rs`, `relation.rs` — Memory entities and relationships
- `space.rs`, `tenant.rs` — Multi-tenancy and space hierarchy
- `profile.rs` — User profile model
- `types.rs` — Shared domain types
- `error.rs` — Domain error types

### store/ — Vector Storage Layer
- `manager.rs` — StoreManager: creates per-tenant LanceDB connections
- `lancedb.rs` — LanceDB 0.27 operations (vector CRUD, search)
- `tenant.rs` — TenantStore: tenant metadata persistence
- `spaces.rs` — SpaceStore: space membership and metadata

### embed/ — Embedding Services
- `service.rs` — `EmbedService` trait + `create_embed_service()` factory
- `openai_compat.rs` — OpenAI-compatible embedding API client
- `bedrock.rs` — AWS Bedrock embedding (feature-gated)
- `noop.rs` — No-op embedding for testing

### llm/ — LLM Services
- `service.rs` — `LlmService` trait + factory functions (primary + recall)
- `openai_compat.rs` — OpenAI-compatible chat completion client
- `bedrock.rs` — AWS Bedrock LLM (feature-gated)
- `noop.rs` — No-op LLM for testing

### retrieve/ — Hybrid Retrieval
- `pipeline.rs` — Multi-stage retrieval pipeline
- `reranker.rs` — Result reranking logic
- `trace.rs` — Retrieval tracing and debugging

### multimodal/ — Multi-format Processing
- `service.rs` — Multimodal orchestrator
- `code.rs` — Code analysis via tree-sitter AST (Rust, Python, JS, TS)
- `pdf.rs` — PDF text extraction (pdf-extract)
- `image.rs` — Image processing
- `video.rs` — Video processing

### lifecycle/ — Memory Lifecycle
- `scheduler.rs` — `LifecycleScheduler`: periodic decay/forgetting/upgrade cycles
- `decay.rs` — Weibull decay model for memory relevance scoring
- `forgetting.rs` — Auto-forgetting low-relevance memories
- `tier.rs` — Memory tier upgrade/downgrade logic

### profile/ — User Profiling
- `service.rs` — Auto-generates user profiles from accumulated memories

### connectors/ — External Integrations
- `github.rs` — GitHub API integration

## Plugins (TypeScript)

### opencode/ (9 source files)
OpenCode platform memory plugin. Files: client, config, hooks, index, keywords, logger, privacy, tags, tools.

### openclaw/ (7 source files)
OpenClaw agent plugin. Files: client, context-engine, hooks, index, server-backend, tools, types.

### mcp/ (3 source files)
MCP server implementation. Files: client, index, tools.

### claude-code/
Claude Code hooks + skills integration (no src/ — uses hooks/ and skills/ directories).

## Configuration

All config via environment variables (`config.rs`):

| Variable | Default | Description |
|----------|---------|-------------|
| `OMEM_PORT` | `8080` | HTTP listen port |
| `OMEM_LOG_LEVEL` | `info` | Tracing log level |
| `OMEM_EMBED_PROVIDER` | `noop` | Embedding provider: noop/openai/bedrock |
| `OMEM_EMBED_API_KEY` | (empty) | Embedding API key |
| `OMEM_EMBED_BASE_URL` | (empty) | Embedding API base URL |
| `OMEM_EMBED_MODEL` | (empty) | Embedding model name |
| `OMEM_LLM_PROVIDER` | (empty) | LLM provider: noop/openai/bedrock |
| `OMEM_LLM_API_KEY` | (empty) | LLM API key |
| `OMEM_LLM_MODEL` | `gpt-4o-mini` | LLM model name |
| `OMEM_LLM_BASE_URL` | `https://api.openai.com` | LLM API base URL |
| `OMEM_LLM_RESPONSE_FORMAT` | (none) | Optional response format |
| `OMEM_RECALL_LLM_PROVIDER` | (empty) | Recall LLM provider (separate from primary) |
| `OMEM_RECALL_LLM_API_KEY` | (empty) | Recall LLM API key |
| `OMEM_RECALL_LLM_MODEL` | (empty) | Recall LLM model |
| `OMEM_RECALL_LLM_BASE_URL` | (empty) | Recall LLM base URL |
| `OMEM_S3_BUCKET` | (empty) | S3 bucket for storage (uses local if empty) |
| `OMEM_OSS_BUCKET` | (empty) | OSS bucket (takes priority over S3) |
| `OMEM_SCHEDULER_INTERVAL_SECS` | `21600` (6h) | Lifecycle scheduler interval |
| `OMEM_SCHEDULER_RUN_ON_START` | `true` | Run scheduler on startup |

Storage resolution: OSS > S3 > local `./omem-data`

## Deployment

```bash
# Docker Compose (omem-server + minio)
docker-compose up -d

# Services:
#   omem-server  :8080 (health: GET /health)
#   minio        :9000 (S3 API)  :9001 (Console)
```

Production: `docker-compose.prod.yml`. Requires `.env` file with all OMEM_* variables.

## Key Dependencies (Cargo.toml)

- **HTTP**: axum 0.8, tokio (full), tower-http 0.6 (trace, cors)
- **Vector DB**: lancedb 0.27, arrow 57, lance-index 3.0
- **LLM/Embed**: reqwest 0.13 (rustls, no aws-lc-sys), aws-sdk-bedrockruntime (optional)
- **Multimodal**: tree-sitter 0.24 (Rust/Python/JS/TS grammars), pdf-extract 0.10
- **TLS**: rustls with ring provider (avoids musl SEGV from aws-lc-sys)
- **Dev**: tower 0.5, http-body-util 0.1

## Build & Test Commands

```bash
# Build
cargo build                          # Debug build
cargo build --release                # Release build
cargo build --no-default-features    # Build without Bedrock support

# Run
cargo run                            # Start server (default :8080)

# Test
cargo test                           # All inline tests (47 source files)
cargo test -p omem-server            # Package-specific

# Lint
cargo clippy                         # Rust linter

# Plugins (each)
cd plugins/<name> && npm run build   # Build plugin
cd plugins/<name> && npm test        # Test plugin
```

## Security Rules

### Rust Server

- **NEVER** use `panic!`/`unwrap()` in production code — use `anyhow::Result` or custom error enums. Unwrap only in `#[cfg(test)]` blocks.
- LLM prompts **must** preserve input language — never force translation.
- Privacy detection **must** run during ingestion (`privacy.rs`).
- Space ID normalization prevents path traversal attacks.
- SQL escaping **must** use `escape_sql()`.
- CJK character URL-encoding safety must be maintained.
- Content sanitization: strip XML tags, truncate to prevent HTTP 414.

### TypeScript Plugins

- Follow dependency constraints in each plugin's `package.json`.
- OpenCode plugin: ESM imports must include `.js` extension.
