# Cerebro — Claude Code Plugin

Persistent memory for Claude Code — memories survive across sessions, projects, and machines.

## Installation

```bash
# In Claude Code:
/plugin marketplace add mingxy-cerebro/cerebro-server
/plugin install cerebro@cerebro
```

Restart Claude Code after installation.

## Setup

Set your Cerebro API key. Two options:

**Option A — Environment variables** (recommended, add to `~/.claude/settings.json`):

```json
{
  "env": {
    "OMEM_API_KEY": "your-api-key",
    "OMEM_API_URL": "https://www.mengxy.cc"
  }
}
```

**Option B — Config file** (`~/.config/cerebro/config.json`):

```json
{
  "connection": { "apiUrl": "https://www.mengxy.cc", "apiKey": "your-api-key" }
}
```

**Priority**: env var > config.json > builtin default (`https://www.mengxy.cc`)

Get a free API key:

```bash
curl -X POST https://www.mengxy.cc/v1/tenants \
  -H "Content-Type: application/json" -d "{}"
```

## How It Works

### Hook Events

| Hook | Trigger | What It Does | Timeout |
|------|---------|-------------|---------|
| **SessionStart** | New session | Injects user profile + recent memories + time. Shows connection status via `systemMessage`. | 15s |
| **UserPromptSubmit** | Each user message | Injects reasoned-recall instruction + keyword nudges. POSTs recall-event to web UI. Zero-blocking (no API search). | 5s |
| **PreCompact** | Before context compaction | Flushes conversation delta to server (main ingest point). | 30s |
| **SessionEnd** | Session closes | Final flush of uncommitted conversation delta (backup ingest point). | 30s |
| **PreToolUse** | Before Skill/Bash tools | Recall-approve audit. | 10s |

### Memory Ingest Architecture

Session conversations are saved at two strategic points — **not** every turn:

```
User talks → [N turns] → PreCompact (flush #1) → Context compressed → [N turns] → SessionEnd (flush #2)
                         ↑ Main: saves all delta              ↑ Backup: saves remaining delta
```

- **Cursor-based dedup**: Each session tracks a cursor (last flushed UUID). Only new messages are sent.
- **Smart filtering**: Inject-echo tags (`<cerebro-*>`, `<system-reminder>`) stripped, thinking blocks dropped, tool results truncated.
- **Cost-efficient**: Server-side LLM extraction runs only on flush (typically 1-2 times per session), not per turn.

### Memory Recall Architecture

- **SessionStart**: Three-way parallel injection (profile + recent memories + semantic search), truncated to 10K chars.
- **UserPromptSubmit**: Zero-blocking — only injects a local text instruction telling Claude *when* to search (via `memory-search` skill) and *when* to save (via `memory-save` skill). No API call, no 20s delay.
- **Keyword nudges**: Detects "记住"/"remember" → save nudge; "之前"/"之前"/"recall" → search nudge.

### User Awareness

Cerebro shows a status line at session start via Claude Code's `systemMessage`:

```
🧠 Cerebro v0.3.0 · Connected · 5 memories · Profile ✓
```

### MCP Tools (on-demand)

The plugin bundles the `@ourmem/mcp` server:

| Tool | Purpose |
|------|---------|
| `memory_store` | Save facts, decisions, preferences |
| `memory_search` | Semantic + keyword hybrid search |
| `memory_get` | Retrieve memory by ID |
| `memory_update` | Modify existing memory |
| `memory_delete` | Remove a memory |
| `memory_profile` | View induced user-preference profile |

### Skills

| Skill | Trigger |
|-------|---------|
| `/cerebro:memory-search` | Semantic search by natural-language query |
| `/cerebro:memory-save` | Manually save a memory |
| `/cerebro:memory-profile` | View user-preference profile |

## Configuration Reference

| Env Var | Config Key | Default | Description |
|---------|-----------|---------|-------------|
| `OMEM_API_KEY` | `connection.apiKey` | — | **Required**. API key for Cerebro Server. |
| `OMEM_API_URL` | `connection.apiUrl` | `https://www.mengxy.cc` | Server URL. |
| `MEM_RECENT_COUNT` | `injection.recentCount` | `8` | Recent memories to inject at SessionStart. |
| `MEM_SEARCH_COUNT` | `injection.searchCount` | `8` | Search results at SessionStart. |
| `MEM_MAX_CONTENT` | `content.maxContentLength` | `3000` | Max chars per memory content. |
| `MEM_LOG_ENABLED` | `logging.logEnabled` | `true` | Write logs to `~/.config/cerebro/logs/`. |

## Testing

```bash
# Run all tests (zero dependencies, uses Node.js built-in test runner)
node --test plugins/claude-code/tests/
```

## Requirements

- Node.js 18+
- `OMEM_API_KEY` environment variable

## Plugin Structure

```
plugins/claude-code/
├── .claude-plugin/
│   └── plugin.json              # Plugin manifest
├── .mcp.json                    # MCP server config
├── package.json                 # Version (single source of truth)
├── hooks/
│   ├── hooks.json               # Hook event registration
│   ├── common.mjs               # Shared library (config, HTTP, ingest, injection)
│   ├── session-start.mjs        # SessionStart: profile + recent + status toast
│   ├── user-prompt-submit.mjs   # UserPromptSubmit: recall instruction + nudges
│   ├── pre-compact.mjs          # PreCompact: flush delta before compaction
│   ├── session-end.mjs          # SessionEnd: final flush
│   └── recall-approve.mjs       # PreToolUse: audit hook
├── tests/
│   ├── common.test.mjs          # Unit tests (cleanText, formatRelativeAge, etc.)
│   └── hooks.test.mjs           # Contract tests (stdin→stdout per hook)
├── scripts/
│   └── web-server.mjs           # Local web UI server
└── skills/
    ├── memory-search/
    ├── memory-save/
    └── memory-profile/
```

## License

Apache-2.0
