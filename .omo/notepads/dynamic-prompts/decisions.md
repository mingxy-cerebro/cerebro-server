## Decisions

### Approach: Split const strings + dynamic injection
- Split hardcoded const prompt strings into `_BEFORE_CATS` / `_AFTER_CATS` parts
- Build dynamic category sections at runtime from `&[CategoryConfig]`
- Concatenate parts in builder functions: `format!("{}{}{}", before, dynamic_section, after)`

### Category propagation pattern
- `IngestPipeline` stores `Arc<CategoryRegistry>` and fetches `Vec<CategoryConfig>` once per ingest request
- Categories passed as `&[CategoryConfig]` to all prompt builder functions
- Reconciler already had `registry` + `tenant_id` from prior task — just needed to call `get_active_categories()` and pass to prompt builder

### Session prompts (compress + extract)
- Used placeholder strings (`{SESSION_COMPRESS_CATEGORY_PLACEHOLDER}`) and `str::replace()` for injection
- This avoids splitting large r##"..."## raw strings into multiple const pieces

### Files NOT modified (per constraints)
- `domain/category.rs` — already complete from Task 3
- `store/sqlite.rs` / `store/sqlite_schema.rs` — not touched
- Handler files other than `memory.rs` — not touched
