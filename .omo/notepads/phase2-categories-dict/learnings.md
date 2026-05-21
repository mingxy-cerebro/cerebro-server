# Learnings — phase2-categories-dict

## Project Context
- Rust HTTP server (axum 0.8 + lancedb 0.27)
- No existing SQLite usage — all storage is LanceDB
- rusqlite needs to be added to Cargo.toml
- AppState has 16 fields in api/server.rs
- Category is already a String newtype (Phase 0 done)

## Conventions
- Store init: Two-step pattern `new()` + `init_table()` (see tenant.rs, spaces.rs)
- Error type: `OmemError::Storage(msg)` for all storage errors
- API handlers: Follow spaces.rs/clusters.rs CRUD pattern
- Tests: Inline `#[cfg(test)]` + `#[tokio::test]`, manual mocks (no mockall)
