# Cerebro for ZCode

Persistent memory plugin for ZCode — session-start memory injection, reasoned-recall instruction + keyword nudge on every prompt, recall auto-approval, and incremental session archival at every turn end. Powered by the [Cerebro (omem)](https://github.com/mingxy-cerebro/cerebro-server) backend.

Feature-parity port of the claude-code plugin (`@mingxy/cerebro-claude-code`) and the opencode plugin (`@mingxy/cerebro`), adapted to ZCode's plugin architecture.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  ZCode session                                                       │
│                                                                      │
│  SessionStart (startup|resume|clear|compact)                         │
│    session-start.js → [CEREBRO-MEMORY] injection:                    │
│      [CEREBRO-TIME] + profile + project recent + semantic search     │
│                                                                      │
│  UserPromptSubmit (every message, ZERO API calls)                    │
│    user-prompt-submit.js → <cerebro-recall> reasoning instruction    │
│      + save/recall keyword nudges (config-driven)                    │
│                                                                      │
│  PreToolUse (matcher Skill|Bash)                                     │
│    pre-tool-use.js → auto-approve memory-recall calls                │
│                                                                      │
│  Stop (every turn end)                                               │
│    stop.js → incremental session archive                             │
│      (turnId cursor + message-count slicing, CC-grade cleaning)      │
│                                                                      │
│  MCP tools (17, zero-dependency)                                     │
│    mcp/server.js → memory_* / space_* on-demand                      │
└──────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                   ┌─────────────────────┐
                   │  Cerebro backend    │
                   │  /v1 /v2 API        │
                   └─────────────────────┘
```

### Design notes

- **Per-message keyword search + auto-inject is NOT implemented** — that is the deprecated opencode `autoRecall` pattern. Instead, every user prompt gets a static reasoning instruction (`<cerebro-recall>`) that teaches the model when to call `memory_search` itself. Zero search API calls on the prompt path.
- **ZCode has no PreCompact/PostCompact/SessionEnd events.** Compaction is covered by SessionStart(compact) re-injection plus the Stop-hook snapshot-shrink detection (a compact reset triggers a full re-sync ingest, doubling as compact-summary capture). Ctrl+C is covered by per-turn incremental archival — nothing is left unflushed.
- **Cross-world path canonicalization**: memory producers run in WSL (`/mnt/c/...`) and Windows (`C:\...`) over the same repos. Every `project_path` and project tag hash sent to the server is canonicalized to the `/mnt/<drive>/...` form, so WSL-side (opencode / claude-code) and Windows-side (zcode) plugins share one project identity. Controlled by `connection.projectPathStyle` (`auto` | `wsl` | `native`, default `auto` — POSIX paths pass through untouched, so the same code is safe inside WSL).

## Capabilities vs the other Cerebro plugins

| Feature | opencode | claude-code | zcode (this) |
|---|---|---|---|
| Session-start injection (profile + project + semantic) | ✅ | ✅ | ✅ |
| Time header `[CEREBRO-TIME]` | ✅ (system transform) | ✅ | ✅ (SessionStart) |
| Reasoned-recall instruction per prompt | ❌ | ✅ | ✅ |
| Save/recall keyword nudge | ✅ | ✅ | ✅ |
| Recall call auto-approval | — | ✅ (PreToolUse) | ✅ (PreToolUse) |
| Periodic session archival | ✅ session.idle | ✅ Stop×5 + SessionEnd | ✅ Stop (per-turn incremental) |
| Compact summary ingestion | ✅ | ✅ Pre/PostCompact | ✅ Stop shrink-detection |
| Ingest cleaning (strip echo, drop thinking, truncate tools) | partial | ✅ | ✅ |
| Container tags (user/project isolation) on writes | ✅ | ✅ | ✅ |
| 17 memory/space tools | ✅ plugin tools | ✅ MCP | ✅ MCP (zero-dep) |
| Auto-store toggle | ✅ | — | ✅ (memory_toggle + Stop gate) |
| Web UI | ✅ | ✅ | ✅ (127.0.0.1:5212, idle daemon) |
| Per-message keyword auto-inject (deprecated) | ✅ deprecated | ❌ | ❌ |

## Hooks

| Hook | Event | Purpose |
|------|-------|---------|
| `session-start.js` | `SessionStart` (`startup\|resume\|clear\|compact`) | Inject `[CEREBRO-MEMORY]` once at session start: time header + profile + recent project memories + semantic search (query derived from the last user message on resume/compact; project name on cold start) |
| `user-prompt-submit.js` | `UserPromptSubmit` | Inject `<cerebro-recall>` reasoning instruction + keyword nudges (static text, no API calls) |
| `pre-tool-use.js` | `PreToolUse` (`Skill\|Bash`) | Auto-approve memory-recall calls (Bash path guarded against shell-chaining injection) |
| `stop.js` | `Stop` | Incremental archive via `/v1/memories/session-ingest` |

> ZCode supports exactly seven hook events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PostToolUseFailure`, `Stop`. `PreCompact` / `PostCompact` / `SessionEnd` / `SubagentStop` are **not** supported (their duties are covered as described under Design notes).

## MCP Tools (17, zero-dependency)

The bundled `mcp/server.js` implements the stdio JSON-RPC transport inline — no `npm install`, no node_modules. Works straight from a marketplace / git / directory install. Loaded via the plugin's own `.mcp.json`.

Memory: `memory_store` · `memory_search` · `memory_get` · `memory_update` · `memory_delete` · `memory_list` · `memory_ingest` · `memory_stats` · `memory_profile` · `memory_profile_stats` · `memory_toggle`
Spaces: `space_create` · `space_list` · `space_add_member` · `memory_share` · `memory_pull` · `memory_reshare`

Business rules aligned with the other plugins: every write auto-prepends container tags (`omem_user_*` / `omem_project_*`), searches auto-scope by the same tags, `source` defaults to `zcode`, query capped at 200 chars, content sanitized at 3000, enums (category / visibility / scope / mode / role) validated before any network call.

## Skills & Commands

- Skill `memory-recall` — triggers on "搜/记得/之前/search/recall"
- Skill `memory-store` — triggers on "记住/保存/别忘了/save this"
- `/memory <text>` — store a memory
- `/recall <query>` — search memories
- `/memory-save` — summarize and archive the current session (via `memory_ingest`)

## Installation

Requires Node.js ≥ 18. No npm dependencies.

### Option A: ZCode marketplace (recommended)

1. In ZCode → Settings → Plugin Management → **Discover** tab → **`+`** button.
2. Add this GitHub repository: `mingxy-cerebro/cerebro-server` (the repo root ships `marketplace.json`).
3. Find **Cerebro Memory** in the marketplace → **Get**.
4. Restart ZCode.

Updates track the repo. Behind a proxy, set `ZCODE_HTTP_PROXY=http://host:port` (a bare `http_proxy` is ignored).

### Option B: npm

```sh
npm install -g @mingxy/cerebro-zcode
# postinstall copies the plugin to ~/.zcode/plugins/cerebro and registers
# it via plugins.dirs; restart ZCode afterwards.
```

Uninstall: `npm uninstall -g @mingxy/cerebro-zcode` (preuninstall removes files + config entries).

### Option C: dev sync (from a repo checkout)

```sh
git clone https://github.com/mingxy-cerebro/cerebro-server.git
cd cerebro-server/plugins/zcode
node install.js                          # sync to ~/.zcode/plugins/cerebro
node install.js --target /custom/path    # sync elsewhere
node install.js --uninstall              # remove
```

The installer also cleans up any pre-0.3.0 global `mcp.servers.cerebro` registration (the plugin's own `.mcp.json` takes over — keeping both would spawn two identical MCP servers).

## Configuration

### Credentials (required)

```bash
# Linux/macOS
export OMEM_API_URL="https://www.mengxy.cc"
export OMEM_API_KEY="your-tenant-key"
```

```cmd
:: Windows (cmd)
set OMEM_API_URL=https://www.mengxy.cc
set OMEM_API_KEY=your-tenant-key
```

Or in `~/.config/cerebro/config.json` (priority: env vars > config file > defaults):

```jsonc
{
  "connection": {
    "apiUrl": "https://www.mengxy.cc",
    "apiKey": "your-tenant-key"
  }
}
```

### Injection-content config (`~/.zcode/cerebro.json`)

Auto-initialized from the bundled `config.default.json` on first run (three-level fallback: user file > bundled > built-in). Controls WHAT gets injected:

| Section | Keys | Default | Description |
|---|---|---|---|
| `language` | — | `en` | Language of injected instructions |
| `recall` | `enabled`, `prompt` | `true` | The `<cerebro-recall>` reasoning instruction |
| `nudge` | `enabled`, `saveKeywords[]`, `recallKeywords[]`, `savePrompt`, `recallPrompt` | see file | Keyword nudges |
| `sessionStart` | `profileEnabled`, `recentActivityEnabled`, `timeEnabled` | all `true` | SessionStart injection toggles |

### Server-connection config reference (`~/.config/cerebro/config.json`)

| Section | Key | Default | Description |
|---------|-----|---------|-------------|
| `connection.apiUrl` | — | `https://www.mengxy.cc` | Cerebro backend URL |
| `connection.apiKey` | — | `""` | Tenant API key |
| `connection.requestTimeoutMs` | `OMEM_REQUEST_TIMEOUT_MS` | `15000` | HTTP timeout |
| `connection.projectPathStyle` | `OMEM_PROJECT_PATH_STYLE` | `auto` | Path canonicalization: `auto` / `wsl` / `native` |
| `content.maxQueryLength` | — | `200` | Search query char cap |
| `content.maxContentChars` | — | `30000` | Total injection char cap |
| `content.maxContentLength` | — | `3000` | Single content char cap |
| `injection.recentCount` | — | `5` | Project memories to inject |
| `injection.searchCount` | — | `10` | Search results to inject |
| `injection.profileTimeoutMs` / `recentTimeoutMs` / `searchTimeoutMs` | — | `2000` / `3000` / `5000` | Per-fetch degraded timeouts |
| `ingest.autoCaptureThreshold` | `OMEM_AUTO_CAPTURE_THRESHOLD` | `5` | Min total messages before archival |
| `ingest.ingestMode` | `OMEM_INGEST_MODE` | `smart` | `smart` (LLM) or `raw` |

## Logs & state

Hook logs: `~/.config/cerebro/logs/cerebro-zcode.log` (5MB rolling, 7-day expiry).
Ingest state: `~/.config/cerebro/zcode-state/` (per-session turn cursors + auto-store switches; or `${ZCODE_PLUGIN_DATA}`).
Web UI: `http://127.0.0.1:5212` (auto-started daemon, idle self-shutdown).

## Development

```sh
cd plugins/zcode
npm test          # node --test tests/ — 45 tests, zero dependencies
```

## License

Apache-2.0
