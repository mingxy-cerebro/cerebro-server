# Decisions — phase2-categories-dict

## Architecture Decisions
- SQLite for config data (categories, aliases) — separate from LanceDB vector storage
- DashMap for in-memory cache (lock-free reads, Mutex only for batch refresh)
- Per-tenant isolation via composite PRIMARY KEY (name, tenant_id)
- Seed data: 9 categories from design doc for new tenants
- OLD 6 categories (profile, entities, events, cases, patterns) → completely deprecated
- No backward compatibility needed — user will use new API key after deployment
