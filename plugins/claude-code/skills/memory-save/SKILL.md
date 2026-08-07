---
name: memory-save
description: Persist a fact, decision, or preference to the user's long-term memory (Cerebro). Use when the user explicitly says "remember / save / store / don't forget / note this", or when you identify important information worth preserving across sessions — preferences, coding style, architecture decisions, bug fixes, project entities, milestones, workflows, or user identity traits. Each memory must be atomic (one fact), self-contained, and precise. Private secrets/credentials/personal data MUST use visibility=private; cross-project knowledge uses scope=global.
---

# Memory Save

Store one atomic, self-contained memory. Before calling, decide:
1. **category** — which of the 6 buckets does this belong to?
2. **scope** — project-specific or cross-project?
3. **visibility** — does it contain sensitive data?
4. **tags** — at least one descriptive snake_case tag.

## Category enum (lowercase, exact)

| Value | Use for |
|-------|---------|
| `cases` (default) | Work records, bug fixes, architecture decisions, troubleshooting notes |
| `preferences` | User likes/dislikes, coding style, tool choices, review habits |
| `entities` | Projects, tools, libraries, people, concepts worth remembering |
| `events` | Time-bound milestones — deployments, releases, incidents, deadlines |
| `profile` | User identity traits — role, skills, team membership, timezone |
| `patterns` | Workflows, methodologies, recurring best practices |

## Visibility

- `global` (default) — all agents can see and recall. Correct for normal work notes.
- `private` — ONLY the current agent sees it. **MUST use for**: passwords, API keys, tokens, DB credentials, SSH keys, personal info (phone/email/address), internal company details, or anything the user would not want other agents to access. When in doubt, ask the user.

## Scope

- `project` (default) — visible only in this project's context (auto `project_path`).
- `global` — visible across all projects. Use for user preferences, general knowledge, cross-project patterns.

## How to run

```bash
bash "$CLAUDE_PLUGIN_ROOT/scripts/memory-save.sh" \
  --content "Fixed memory_type validation bug in memory.rs:1480 — LLM returned illegal 'pinned' value, added match guard normalizing to WORK/EMOTIONAL fallback" \
  --tags "rust_backend,memory_system,bug_fix" \
  --category cases \
  --scope project \
  --visibility global
```

Content can also be piped:

```bash
echo "user prefers terse caveman-style replies" | \
  bash "$CLAUDE_PLUGIN_ROOT/scripts/memory-save.sh" --tags "communication,style" --category preferences --scope global
```

## Writing good content

- BAD: "fixed some bugs"
- GOOD: "Fixed refresh-token rotation bug in auth.rs:230 — replay window allowed token reuse; added nonce cache with 60s TTL."
- BAD: "user likes stuff"
- GOOD: "User prefers dark IDE theme, 2-space indent for Rust, tabs for Go."

## Output

- Success: `ok id=mem_abc123 tags=omem_user_xxx,rust_backend`
- Failure: `error: <reason>`

## Notes

- Content is sanitized (XML tags stripped, whitespace collapsed, truncated to `$MEM_MAX_CONTENT`).
- Container tags (`omem_user_<hash>`, `omem_project_<hash>`) are auto-prepended.
- `project_path` is auto-detected from git toplevel.
- `source` is hardcoded to `claude-code`, `agent_id` to `cerebro`.
