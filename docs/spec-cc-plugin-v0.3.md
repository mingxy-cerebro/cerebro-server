# Cerebro Claude Code Plugin v0.3.0 — Feature Spec

## Problem Statement

Claude Code users lose all context when a session ends — user preferences, project decisions, and past bug-fixing experience vanish. The existing memory system (Cerebro Server) had an opencode plugin, but the Claude Code plugin had four critical issues:

1. **Ingest timing error**: Stop hook triggered session-ingest every turn, causing server-side LLM API cost explosion (20 turns = 20 API calls).
2. **Recall blocking**: UserPromptSubmit did server-side semantic search every turn, blocking 20+ seconds.
3. **Web UI recall retention bug**: recall-events were only POSTed at SessionStart with hardcoded query_text "session start injection" — the Web Sessions page couldn't show actual user conversations.
4. **Zero user awareness**: No toast/notification — users couldn't perceive Cerebro's connection status, version, or ingest activity.

## Solution

A production-grade Claude Code memory plugin using the CC Hook system:
- **Low-frequency, high-efficiency ingest**: Flush deltas only at PreCompact (before compaction) and SessionEnd (session close), not every turn.
- **Zero-blocking recall**: UserPromptSubmit injects only local text instructions (reasoned recall). Claude searches on-demand via memory-search skill.
- **Complete Web UI retention**: Every injection POSTs a recall-event recording the actual user prompt + injected content.
- **User-perceivable**: Connection status and version shown via CC `systemMessage` field.

## User Stories

### Memory Auto-Save (Session Ingest)

1. As a developer, I want session conversations auto-saved to Cerebro Server, so I don't have to manually remember.
2. As a developer, I want saves triggered only at key points (before compaction, at session end), so API costs stay low.
3. As a developer, I want short sessions (no compaction) to still save memories, so brief Q&A isn't lost.
4. As a developer, I want duplicate content filtered, so storage isn't wasted.
5. As a developer, I want inject-echo content (cerebro tags, system-reminder) stripped, so no recursive noise.
6. As a developer, I want thinking blocks dropped, so only meaningful dialogue is saved.
7. As a developer, I want tool_results truncated, so large file contents don't overflow storage.
8. As Cerebro Server, I want ingest requests to include project_name and project_path, so memories are project-isolated.

### Memory Recall (Injection)

9. As a developer, I want my user profile and recent project memories auto-injected at session start, so Claude knows me from the beginning.
10. As a developer, I want UserPromptSubmit to not block 20 seconds for semantic search, so interaction stays fluid.
11. As Claude, I want to receive reasoned-recall instructions, so I can autonomously decide when to search memories.
12. As Claude, when the user says "remember", I want a nudge, so I know to use the memory-save skill.
13. As Claude, when the user says "earlier/last time", I want a nudge, so I know to use the memory-search skill.
14. As a developer, I want time info (CEREBRO-TIME) injected into Claude's context, so Claude has temporal awareness.

### Web UI Recall Retention

15. As a developer, I want the Web Sessions page to show actual user prompts, not hardcoded "session start injection".
16. As a developer, I want every injection's content (profile + recent + search) recorded, so I can trace what was injected.
17. As a developer, I want recall-events to include profile_injected flag, so the Web UI can distinguish profile injections.
18. As a developer, I want recall-event POST to not block hook execution, so slow Web Servers don't affect Claude interaction.

### User Awareness (Toast / systemMessage)

19. As a developer, I want to see Cerebro version, connection status, and memory count at session start, so I know the memory system is working.
20. As a developer, I want to see whether Profile loaded successfully (✓/✗), so I can diagnose config issues.
21. As a developer, when API key is missing, I want a clear error with instructions to get a key, so I can get started quickly.

### Standardized Loading (Marketplace)

22. As a developer, I want to install via `/plugin marketplace add mingxy-cerebro/cerebro-server`, so I don't need to copy files manually.
23. As a developer, I want semantic version numbers, so I can track updates.
24. As a new user, I want a README telling me how to configure and use the plugin, so I can start without friction.

### Configuration & Fault Tolerance

25. As a developer, I want to override config via env vars (OMEM_API_KEY, OMEM_API_URL), so I don't need to edit config files.
26. As a developer, I want config cascade (env > config.json > defaults), for flexibility.
27. As a developer, when network times out, I want hooks to degrade gracefully, so Claude is unaffected.
28. As a developer, when session-ingest fails, I want the cursor to not advance, so it retries next time.
29. As a developer, I want logs written to `~/.config/cerebro/logs/claude-code.log`, so I can troubleshoot.

## Implementation Decisions

### Hook Event Architecture

| Hook Event | Actions | Timeout | Blocking |
|------------|---------|---------|----------|
| **SessionStart** | Profile + recent injection + POST recall-event + web server launch + systemMessage status | 15s | Blocking (first injection) |
| **UserPromptSubmit** | Reasoned-recall instruction + keyword nudge + POST recall-event | 5s | Blocking (local text + lightweight POST, typically <500ms) |
| **PreToolUse (Skill\|Bash)** | Recall-approve audit | 10s | Blocking |
| **PreCompact** | flushSessionIngest (save all delta before compaction) | 30s | Blocking |
| **SessionEnd** | flushSessionIngest (backup final flush) | 30s | Blocking but cannot prevent termination |

**Key decision**: Deleted Stop hook (Q3=A). Stop triggered ingest every turn causing server-side LLM API cost linear with turn count. Replaced with PreCompact + SessionEnd dual flush points.

### Session Ingest Strategy

**Dual-point delta flush** (replaces per-turn Stop):
1. PreCompact — main flush: saves all unflushed delta before compaction.
2. SessionEnd — backup flush: saves delta for short sessions that didn't trigger compaction.

**Cursor mechanism**: Each session has a cursor (UUID) pointing to the last flushed transcript entry. Flush sends only delta past the cursor; advances cursor on success. Failure doesn't advance — retries next time.

**Filtering rules** (cleanText):
- Strip `<system-reminder>`, `<cerebro-*>`, `<supermemory-*>` tags (inject echo).
- Drop thinking blocks.
- Truncate tool_result to 500 chars, tool_use to 100 chars.
- Skip entries with final text < 100 chars.

### Recall Strategy

**SessionStart injection** (buildMemoryInjection):
- Three-way parallel: profile (`/v2/profile/inject`) + recent (`/v1/memories`) + search (skipped when query is empty).
- Profile timeout: 2s, recent timeout: 3s, search timeout: 5s.
- Total output truncated to 10,000 chars (CC additionalContext limit).
- CEREBRO-TIME embedded in additionalContext (Claude needs temporal awareness).

**UserPromptSubmit injection** (Q4=A):
- Pure local text: reasoned-recall instruction + keyword nudge.
- Zero API search calls (eliminates 20s+ blocking).
- Claude searches on-demand via memory-search skill.

**systemMessage Toast** (Q2):
- CEREBRO-STATUS shown to user via CC `systemMessage` field.
- Content: `🧠 Cerebro v{version} · Connected · {N} memories · Profile ✓/✗`
- API key missing → error message with instructions.

### Recall Event Retention

Every injection (SessionStart + UserPromptSubmit) POSTs to `/v1/recall-events`:
- `session_id`, `recall_type` ("session_start" | "auto"), `query_text` (user's actual prompt, truncated 500 chars), `profile_injected`, `kept_count`, `injected_content` (truncated 10,000 chars).
- 5s timeout, fire-and-forget (POST failure doesn't affect hook).

### Config Cascade

```
Environment variables (highest priority)
  ↓ fallback
~/.config/cerebro/config.json (sections: connection/content/injection/ingest/logging)
  ↓ fallback
Builtin defaults
```

### Version Management

- `package.json` is the single source of truth for version.
- `common.mjs` reads `PLUGIN_VERSION` from `package.json` at runtime.
- `.claude-plugin/plugin.json` and root `.claude-plugin/marketplace.json` versions synced manually.
- Current version: 0.3.0

### PreCompact Schema Limitation

CC's PreCompact event does **not** support `hookSpecificOutput.additionalContext`. The hookSpecificOutput wrapper only supports: PreToolUse, UserPromptSubmit, PostToolUse, PostToolBatch, Stop/SubagentStop. PreCompact can only return top-level fields (`{}`, `systemMessage`, `decision`, etc.). Compaction guidance is covered by the cerebro-recall instruction in CLAUDE.md, not injected via PreCompact.

## Testing Decisions

### Testing Principles

- Test external behavior only (stdin→stdout contract), not implementation details.
- Mock network requests (no real Cerebro Server dependency).
- Each hook script as an independent test unit.

### Test Modules

| Module | Test Seam | Coverage |
|--------|-----------|----------|
| `common.test.mjs` | Unit (direct import) | cleanText, formatRelativeAge, truncateQuery, sanitizeContent |
| `hooks.test.mjs` | Contract (spawn + stdin→stdout) | user-prompt-submit (normal/save/recall keywords), pre-compact (empty output), session-end (empty output, missing fields) |

### Test Framework

Node.js built-in `node:test` + `node:assert` (zero dependency). Network mocked by pointing `OMEM_API_URL` to a dead port (`http://127.0.0.1:1`) — fetch calls fail fast, error-catching in hook code swallows failures.

## Out of Scope

- **PostCompact summary storage** (Q1=C): CC's compact summary is not separately stored. Covered by cerebro-recall instruction.
- **Async hook approach** (Q4=B/C): No async UserPromptSubmit. Delayed injection has prompt injection risk + context mismatch.
- **opencode plugin alignment**: opencode's timeMemorySystemHook, autocontinueHook are out of scope.
- **Server-side changes**: API endpoints are out of scope.
- **Web UI frontend changes**: Sessions page frontend is out of scope.

## Further Notes

### Known Limitations

1. **SessionEnd flush reliability**: CC SessionEnd cannot prevent session termination; hook process is killed on timeout. flushSessionIngest's POST typically completes in <1s (server processes LLM extraction asynchronously); 30s timeout is sufficient.
2. **UserPromptSubmit POST recall-event latency**: postRecallEvent has a 5s timeout, typically <500ms. If Web Server is unreachable, POST silently fails — doesn't affect Claude interaction.
3. **Cursor persistence**: Cursor stored at `~/.config/cerebro/trackers/{sessionId}.txt`. No cleanup mechanism (low priority, single file is a few bytes).

### Distribution

```bash
/plugin marketplace add mingxy-cerebro/cerebro-server
/plugin install cerebro@cerebro

export OMEM_API_KEY="your-key"
# Optional: custom server
export OMEM_API_URL="https://your-server.com"
```
