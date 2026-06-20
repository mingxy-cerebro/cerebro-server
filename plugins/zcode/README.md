# Cerebro for ZCode

Persistent memory plugin for ZCode — auto-inject user preferences, project memories, and global memories at session start; auto-archive sessions at stop/compact. Powered by the [Cerebro (omem)](https://github.com/mingxy-cerebro/cerebro-server) backend.

This plugin ports the core capabilities of the OpenCode plugin (`@mingxy/cerebro`) to ZCode's plugin architecture.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  ZCode session                                                  │
│                                                                 │
│  ┌─────────────────┐   SessionStart    ┌──────────────────────┐ │
│  │                 │ ───────────────▶  │ session-start.js     │ │
│  │   AI agent      │                   │  • profile inject    │ │
│  │                 │                   │  • project memories  │ │
│  │                 │ ◀──── inject ──── │  • global search     │ │
│  │                 │                   └──────────────────────┘ │
│  │                 │                                            │
│  │                 │   Stop            ┌──────────────────────┐ │
│  │                 │ ───────────────▶  │ stop.js              │ │
│  │                 │                   │  • session-ingest    │ │
│  │                 │                   │  • idempotent dedup  │ │
│  │                 │                   └──────────────────────┘ │
│  │                 │                                            │
│  │                 │   PreCompact     ┌──────────────────────┐ │
│  │                 │ ───────────────▶ │ pre-compact.js       │ │
│  │                 │ ◀──── inject ─── │  • compaction ctx    │ │
│  │                 │                   │  • archive messages  │ │
│  │                 │   ┌───────────┐   └──────────────────────┘ │
│  │                 │   │ MCP tools │                            │
│  │                 │ ─▶│ (17)      │◀── AI on-demand calls     │
│  └─────────────────┘   └───────────┘                            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                   ┌─────────────────────┐
                   │  Cerebro backend    │
                   │  (omem-server)      │
                   │  /v1 /v2 API        │
                   └─────────────────────┘
```

## Capabilities

### Hooks (automatic, via Node scripts)

| Hook | Event | Purpose |
|------|-------|---------|
| `session-start.js` | `SessionStart` (startup\|clear\|compact) | **Core**: inject user preferences (profile) + project memories + global memories once at session start |
| `stop.js` | `Stop` | Auto-archive the session via `/v1/memories/session-ingest` (idempotent, deduped) |
| `pre-compact.js` | `PreCompact` | Inject 6-section compaction context + archive recent messages |

> **Note**: ZCode has no per-message hook. The OpenCode plugin's per-message recall is deprecated. This plugin injects memory **once** at session start (the documented correct behavior) instead of on every message.

### MCP Tools (17 tools, AI on-demand)

Powered by [`@ourmem/mcp`](https://www.npmjs.com/package/@ourmem/mcp) by default (zero-setup). An enhanced `mcp/server.js` with v2 profile support is also bundled.

Memory: `memory_store` · `memory_search` · `memory_get` · `memory_update` · `memory_delete` · `memory_list` · `memory_ingest` · `memory_stats` · `memory_profile` · `memory_profile_stats` · `memory_toggle`
Spaces: `space_create` · `space_list` · `space_add_member` · `memory_share` · `memory_pull` · `memory_reshare`

### Skills (lightweight shortcuts)

- `memory-recall` — triggers on "搜/记得/之前/search/recall"
- `memory-store` — triggers on "记住/保存/别忘了/save this"

### Commands (slash commands)

- `/memory <text>` — store a memory
- `/recall <query>` — search memories

## Installation

> **How this works**: ZCode plugins are filesystem-based (not in-process JS modules), so ZCode has no built-in `npm install <plugin>` command. This package bridges that gap with an npm **postinstall** hook: `npm install -g @ourmem/zcode` copies the plugin to `~/.zcode/plugins/cerebro/` and registers it in `~/.zcode/cli/config.json` under `plugins.dirs`. Restarting ZCode then auto-loads it (`source:"inline"`, `defaultEnabled:true`). The ergonomics match OpenCode's `"plugin": ["@pkg"]` — one command, restart, done.

### Option A: npm (recommended)

```sh
npm install -g @ourmem/zcode
# postinstall hook runs automatically → plugin registered
# then restart ZCode
```

Uninstall is symmetric:
```sh
npm uninstall -g @ourmem/zcode
# preuninstall hook removes plugin files + config entry
```

The postinstall hook is a no-op during local development (when the package isn't inside a `node_modules/` tree), so `npm install` in this repo won't self-install.

### Option B: Manual one-shot installer

```sh
git clone https://github.com/mingxy-cerebro/cerebro-server.git
cd cerebro-server/plugins/zcode
node install.js                          # install to ~/.zcode/plugins/cerebro
node install.js --target /custom/path    # install elsewhere
node install.js --uninstall              # remove
```

### Option C: Manual config-driven install

1. Place the plugin anywhere stable, e.g. `~/.zcode/plugins/cerebro/`.
2. Add its path to `plugins.dirs` in `~/.zcode/cli/config.json`:
   ```jsonc
   {
     "plugins": {
       "dirs": ["C:\\Users\\you\\.zcode\\plugins\\cerebro"],
       "enabled": true
     }
   }
   ```
3. Restart ZCode.

### Supported hook events

ZCode only supports these hook events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop` (verified from the ZCode runtime's `Kr` whitelist). This plugin uses `SessionStart` (memory injection) and `Stop` (session archival). Note: `PreCompact`/`SessionEnd`/`SubagentStop` are **not** supported and will trigger `plugin_hook_unsupported_event` errors.

## Configuration

### Credentials (required)

Set the Cerebro API URL and key as environment variables:

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

Or in `~/.config/cerebro/config.json`:

```jsonc
{
  "connection": {
    "apiUrl": "https://www.mengxy.cc",
    "apiKey": "your-tenant-key"
  }
}
```

Priority: env vars > config file > defaults.

### Full config reference

| Section | Key | Default | Description |
|---------|-----|---------|-------------|
| `connection.apiUrl` | — | `https://www.mengxy.cc` | Cerebro backend URL |
| `connection.apiKey` | — | `""` | Tenant API key |
| `connection.requestTimeoutMs` | `OMEM_REQUEST_TIMEOUT_MS` | `15000` | HTTP timeout |
| `content.maxQueryLength` | — | `200` | Search query char cap |
| `content.maxContentChars` | — | `30000` | Total injection char cap |
| `content.maxContentLength` | — | `3000` | Single content char cap |
| `injection.recentCount` | — | `5` | Project memories to inject |
| `injection.searchCount` | — | `10` | Search results to inject |
| `injection.recentTruncateChars` | — | `0` (no trunc) | Project memory truncation |
| `injection.searchTruncateChars` | — | `0` (no trunc) | Search result truncation |
| `ingest.autoCaptureThreshold` | `OMEM_AUTO_CAPTURE_THRESHOLD` | `5` | Min messages to trigger archive |
| `ingest.ingestMode` | `OMEM_INGEST_MODE` | `smart` | `smart` (LLM) or `raw` |

## Using the Enhanced MCP server (optional)

By default `.mcp.json` uses `npx @ourmem/mcp` (zero-dependency). To use the bundled enhanced server with v2 profile support, first install deps then edit `.mcp.json`:

```bash
cd plugins/zcode && npm install
```

```json
{
  "mcpServers": {
    "cerebro": {
      "command": "node",
      "args": ["${CLAUDE_PLUGIN_ROOT}/mcp/server.js"],
      "env": {
        "OMEM_API_KEY": "${OMEM_API_KEY}",
        "OMEM_API_URL": "${OMEM_API_URL:-https://www.mengxy.cc}"
      }
    }
  }
}
```

## Logs

Hook logs are written to `~/.config/cerebro/logs/cerebro-zcode.log` (5MB rolling, 7-day expiry).

Idempotency state (processed message IDs) lives in `~/.config/cerebro/zcode-state/` (or `${ZCODE_PLUGIN_DATA}`).

## Comparison with the OpenCode plugin

| Feature | OpenCode plugin | This ZCode plugin |
|---------|----------------|-------------------|
| Session-start injection (profile+project+global) | ✅ | ✅ |
| Per-message recall | ✅ (deprecated) | ❌ (intentionally omitted) |
| Stop auto-archive | via `session.idle` | ✅ via `Stop` |
| PreCompact context + archive | ✅ | ✅ |
| 17 memory tools | ✅ | ✅ (MCP) |
| Auto-store toggle | ✅ | ✅ (file-based) |
| Keyword nudge | ✅ | ❌ → Skill fallback |
| TUI / Web UI | ✅ | ❌ |

## License

Apache-2.0
