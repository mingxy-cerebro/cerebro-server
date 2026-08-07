---
name: memory-search
description: Semantic search over the user's long-term memory (Cerebro). Use when the user references past work, prior decisions, stored preferences, or session history you may have saved; when an ambiguous name/concept could be disambiguated by stored context; or when you need background that earlier sessions likely captured. Searches are auto-filtered to the current project + user tags. Returns ranked matches with relevance scores.
---

# Memory Search

Search long-term memory by semantic similarity. Memories are scoped to the current project automatically; global-scope memories are always included.

## When to use

- User mentions "earlier / before / last time / we discussed / remember when …"
- A name, file, or concept feels ambiguous and stored context could resolve it
- You need a prior decision, rationale, or preference before acting
- Recovering context that was compacted away in a previous session

## How to run

```bash
bash "$CLAUDE_PLUGIN_ROOT/scripts/memory-search.sh" "auth flow refresh-token decision" 10
```

- Arg 1: natural-language query (auto-truncated; quoting is recommended).
- Arg 2 (optional): result limit. Defaults to `$MEM_SEARCH_COUNT` (8).

Content can also be piped via stdin:

```bash
echo "why did we pick sqlite over postgres" | bash "$CLAUDE_PLUGIN_ROOT/scripts/memory-search.sh" -
```

## Output format

One line per match:

```
[0.91] mem_abc123: Fixed refresh-token rotation bug in auth.rs — added replay guard
[0.78] mem_def456: Decision: use sqlite for local cache, postgres only on cloud
```

`no memories` when nothing matches. `error: …` on transport/server failure.

## Notes

- Query is auto-truncated to `$MEM_MAX_QUERY_LENGTH` (200 chars).
- Filters by `omem_user_<hash>` and `omem_project_<hash>` tags plus `project_path`, so results respect project isolation.
- For full content of a truncated match, follow up with `memory-save` is NOT the right tool — fetch by id via the server API or re-run search with a sharper query.
