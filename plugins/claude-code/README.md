# Cerebro — Claude Code Plugin

Persistent memory for Claude Code — memories survive across sessions, projects, and machines.

## Installation

### Marketplace (recommended)

```bash
/plugin marketplace add mingxy-cerebro/cerebro-server
```

### Local development

```bash
claude --plugin-dir ./plugins/claude-code
```

## Setup

Configure credentials the same way opencode/zcode do — via the shared config file
`~/.config/cerebro/config.json` (single source of truth across all cerebro plugins):

```json
{
  "connection": { "apiUrl": "https://www.mengxy.cc", "apiKey": "your-api-key" }
}
```

Or set env vars (they override the config file), e.g. in `~/.claude/settings.json`:

```json
{
  "env": {
    "OMEM_API_KEY": "your-api-key",
    "OMEM_API_URL": "https://www.mengxy.cc"
  }
}
```

Priority: env var  >  `~/.config/cerebro/config.json`  >  builtin default
(`https://www.mengxy.cc`). Matches `plugins/opencode/src/config.ts`.

Claude Code auto-injects `env` fields into the process environment.

> **Alternative:** You can also `export OMEM_API_KEY=...` in your shell profile as a fallback.

Self-host Cerebro server (see [deployment guide](../../docs/DEPLOY.md)):

```bash
curl -sX POST http://localhost:8080/v1/tenants \
  -H "Content-Type: application/json" \
  -d '{"name": "my-workspace"}' | jq .api_key
```

## What It Does

### Automatic Hooks

| Hook | Trigger | Effect |
|------|---------|--------|
| **SessionStart** | New session begins | Loads 20 most recent memories and injects them as context |
| **Stop** | Session ends | Sends recent conversation to smart-ingest for automatic memory extraction |
| **PreCompact** | Before context compaction | Saves conversation messages before they're compacted away |

### MCP Tools (on-demand)

The plugin bundles the `@ourmem/mcp` server, giving Claude these tools:

| Tool | Purpose |
|------|---------|
| `memory_store` | Save facts, decisions, preferences |
| `memory_search` | Semantic + keyword hybrid search |
| `memory_get` | Retrieve memory by ID |
| `memory_update` | Modify existing memory |
| `memory_delete` | Remove a memory |

### Skills

| Skill | Trigger |
|-------|---------|
| `/cerebro:memory-search` | Semantic search of memories by natural-language query |
| `/cerebro:memory-save` | Manually save a memory (atomic fact / decision / preference) |
| `/cerebro:memory-profile` | View the induced user-preference profile |

> Legacy `/cerebro:memory-recall` and `/cerebro:memory-store` were removed in favor of the three skills above (they now route through `hooks/common.sh` with proper URL-encoding, sanitization, and project scoping).

## API Endpoints Used

| Endpoint | Method | Used By |
|----------|--------|---------|
| `/v1/memories?limit=20` | GET | SessionStart hook |
| `/v1/memories` | POST | Stop + PreCompact hooks (smart-ingest) |
| `/v1/memories/search?q=...` | GET | memory-search skill |
| `/v1/memories` | POST | memory-save skill |
| `/v2/profile?project_path=...` | GET | memory-profile skill |

## Requirements

- `bash` 4+
- `curl`
- `python3` (for JSON processing in hooks)
- `OMEM_API_KEY` environment variable set

## Plugin Structure

```
plugins/claude-code/
├── .claude-plugin/
│   └── plugin.json          # Plugin manifest
├── .mcp.json                # MCP server config
├── hooks/
│   ├── hooks.json           # Hook event definitions
│   ├── common.sh            # Shared HTTP utilities
│   ├── session-start.sh     # SessionStart hook
│   ├── stop.sh              # Stop hook (smart-ingest)
│   └── pre-compact.sh       # PreCompact hook
├── scripts/
│   ├── memory-search.sh
│   ├── memory-save.sh
│   └── memory-profile.sh
├── skills/
│   ├── memory-search/
│   │   └── SKILL.md
│   ├── memory-save/
│   │   └── SKILL.md
│   └── memory-profile/
│       └── SKILL.md
└── README.md
```
