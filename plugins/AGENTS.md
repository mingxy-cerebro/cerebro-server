# Plugins

## Overview

Cerebro provides **4 TypeScript plugins** that integrate persistent memory into AI agent platforms. Each plugin is a standalone npm package that communicates with the Cerebro REST API.

| Plugin | Platform | Language | Files | Architecture |
|--------|----------|----------|-------|--------------|
| **opencode** | OpenCode | TypeScript | 10 | Client + Hooks + Tools + TUI |
| **openclaw** | OpenClaw | TypeScript | 7 | Client + ContextEngine + Hooks |
| **mcp** | MCP (Cursor/VS Code/Claude Desktop) | TypeScript | 3 | MCP Server (9 tools) |
| **claude-code** | Claude Code | Shell + JSON | 9 | Hooks (bash) + Skills (markdown) |

All plugins share the same configuration pattern: `OMEM_API_URL` + `OMEM_API_KEY` environment variables.

---

## Plugin Matrix

| Name | Package | Files | Purpose | Dependencies | Key Features |
|------|---------|-------|---------|--------------|--------------|
| **opencode** | `@mingxy/cerebro` | 10 src files | OpenCode memory plugin | `@opencode-ai/plugin`, `@opentui/core`, `@opentui/solid`, `solid-js` | Auto-recall, auto-capture, 9 memory tools, clustering TUI |
| **openclaw** | `@ourmem/ourmem` | 7 src files | OpenClaw memory plugin | (none listed) | ContextEngine, 7 lifecycle hooks, memory slot |
| **mcp** | `@ourmem/mcp` | 3 src files | MCP server for any MCP client | `@modelcontextprotocol/sdk`, `zod` | 9 tools, stdio transport, schema validation |
| **claude-code** | (bundled) | 5 hooks + 2 skills + manifest | Claude Code integration | `bash` 4+, `curl`, `python3` | SessionStart/Stop/PreCompact hooks, MCP tools, 2 skills |

---

## opencode/

**Location**: `plugins/opencode/`

OpenCode platform plugin with full TUI (Terminal User Interface) support.

### Source Files (10)

| File | Purpose |
|------|---------|
| `client.ts` | HTTP client — wraps Cerebro REST API |
| `config.ts` | Plugin configuration parsing |
| `hooks.ts` | Lifecycle hooks (session start/end, recall, capture) |
| `index.ts` | Plugin entry point and exports |
| `keywords.ts` | Keyword detection for mid-session recall |
| `logger.ts` | Structured logging utilities |
| `privacy.ts` | Privacy filtering (`<private>` tag handling) |
| `tags.ts` | Tag management and auto-tagging |
| `tools.ts` | Memory tool implementations (store, search, get, etc.) |
| `tui.tsx` | SolidJS-based Terminal UI for memory browsing |

### Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   hooks.ts  │────▶│  client.ts  │────▶│ Cerebro API │
└─────────────┘     └─────────────┘     └─────────────┘
       │
       ▼
┌─────────────┐     ┌─────────────┐
│  tools.ts   │◀────│ keywords.ts │
└─────────────┘     └─────────────┘
       │
       ▼
┌─────────────┐
│   tui.tsx   │  (SolidJS + @opentui)
└─────────────┘
```

### Key Characteristics
- **ESM module** (`"type": "module"`)
- **`.js` extension required** on all ESM imports
- **Exports**: main module + `/tui` subpath
- **OpenCode plugin manifest**: `oc-plugin: ["server", "tui"]`

---

## openclaw/

**Location**: `plugins/openclaw/`

OpenClaw agent plugin with a ContextEngine architecture.

### Source Files (7)

| File | Purpose |
|------|---------|
| `client.ts` | HTTP client for Cerebro API |
| `context-engine.ts` | ContextEngine — manages memory context injection |
| `hooks.ts` | 7 lifecycle hooks |
| `index.ts` | Plugin entry point |
| `server-backend.ts` | Server-side backend integration |
| `tools.ts` | Memory tools exposed to OpenClaw agent |
| `types.ts` | Shared TypeScript types |

### Architecture

```
┌─────────────────┐
│  context-engine │
│    .ts          │
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
┌───────┐ ┌────────┐
│hooks.ts│ │tools.ts│
└───────┘ └────────┘
    │         │
    └────┬────┘
         ▼
    ┌─────────┐
    │client.ts│
    └────┬────┘
         ▼
    ┌─────────┐
    │Cerebro  │
    │  API    │
    └─────────┘
```

### Key Characteristics
- **No external runtime dependencies** (uses OpenClaw's built-in APIs)
- **ContextEngine** pattern: manages what memories to inject into agent context
- **7 lifecycle hooks** for session management

---

## mcp/

**Location**: `plugins/mcp/`

MCP (Model Context Protocol) server implementation. Works with Cursor, VS Code Copilot, Claude Desktop, Windsurf, and any MCP-compatible client.

### Source Files (3)

| File | Purpose |
|------|---------|
| `client.ts` | HTTP client for Cerebro API |
| `index.ts` | MCP server setup, stdio transport, tool registration |
| `tools.ts` | 9 MCP tool definitions with Zod schemas |

### Tools Exposed

| Tool | Description |
|------|-------------|
| `memory_store` | Save a new memory |
| `memory_search` | Semantic search with optional tag filtering |
| `memory_list` | Browse recent memories |
| `memory_ingest` | Ingest conversation messages for smart extraction |
| `memory_get` | Retrieve a memory by ID |
| `memory_update` | Update memory content or tags |
| `memory_forget` | Delete a memory |
| `memory_stats` | Get memory statistics |
| `memory_profile` | View synthesized user profile |

### Key Characteristics
- **Transport**: stdio (MCP standard)
- **Schema validation**: Zod (`zod: ^3.23.0`)
- **SDK**: `@modelcontextprotocol/sdk: ^1.0.0`
- **Binary**: `ourmem-mcp` (via `npx -y @ourmem/mcp`)
- **Build**: `tsc` compiles to `dist/`

---

## claude-code/

**Location**: `plugins/claude-code/`

Claude Code integration using bash hooks and markdown skills. No `src/` directory — entirely shell-script and manifest based.

### Structure

```
plugins/claude-code/
├── .claude-plugin/
│   └── plugin.json          # Plugin manifest
├── .mcp.json                # MCP server config (bundles @ourmem/mcp)
├── hooks/
│   ├── hooks.json           # Hook event definitions
│   ├── common.sh            # Shared HTTP utilities (curl wrappers)
│   ├── session-start.sh     # SessionStart hook — loads 20 recent memories
│   ├── stop.sh              # Stop hook — smart-ingest conversation
│   └── pre-compact.sh       # PreCompact hook — save before compaction
├── skills/
│   ├── memory-recall/
│   │   └── SKILL.md         # /ourmem:memory-recall skill
│   └── memory-store/
│       └── SKILL.md         # /ourmem:memory-store skill
└── README.md
```

### Hooks

| Hook | Trigger | Action |
|------|---------|--------|
| **SessionStart** | New session begins | `GET /v1/memories?limit=20` — injects into context |
| **Stop** | Session ends | `POST /v1/memories` — smart-ingest recent conversation |
| **PreCompact** | Before context compaction | `POST /v1/memories` — save messages before they're lost |

### MCP Tools
The plugin bundles `@ourmem/mcp` via `.mcp.json`, exposing: `memory_store`, `memory_search`, `memory_get`, `memory_update`, `memory_delete`.

### Skills
| Skill | Trigger | Action |
|-------|---------|--------|
| `/ourmem:memory-recall` | Manual | `GET /v1/memories/search?q=...` |
| `/ourmem:memory-store` | Manual | `POST /v1/memories` |

### Requirements
- `bash` 4+
- `curl`
- `python3` (for JSON processing in hooks)
- `OMEM_API_KEY` environment variable

---

## Common Patterns

### 1. Client Pattern
All plugins use a thin HTTP client wrapper around `fetch` that:
- Reads `OMEM_API_URL` and `OMEM_API_KEY` from env
- Sets `X-API-Key` header
- Handles JSON serialization/deserialization
- Provides typed methods matching Cerebro REST endpoints

### 2. Hook Pattern
Platform-specific hooks trigger at session boundaries:
- **Start**: Recall recent memories → inject into agent context
- **End**: Capture conversation → send to `POST /v1/memories` (smart-ingest)
- **Compact**: Preserve messages before context window compression

### 3. Tool Pattern
Each plugin exposes a consistent set of memory operations:
- Store, Search, Get, Update, Delete, List, Ingest, Stats, Profile

### 4. Environment Configuration
All plugins rely on two environment variables:
| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OMEM_API_KEY` | Yes | — | Tenant API key (same as tenant ID) |
| `OMEM_API_URL` | No | `http://localhost:8080` | Cerebro server URL |

---

## Build & Test

### Per-Plugin Commands

```bash
# OpenCode
cd plugins/opencode && npm run build

# OpenClaw
cd plugins/openclaw && npm run build

# MCP
cd plugins/mcp && npm run build   # tsc → dist/

# Claude Code — no build step (shell scripts)
```

### Publishing
- **MCP**: `npm run prepublishOnly` runs `tsc` before publish
- **All packages**: Published to npm registry under `@mingxy/` and `@ourmem/` scopes

---

## Warnings

### 1. No Tests in Any Plugin
None of the 4 plugins contain test files. The `package.json` files do not define test scripts or include test dependencies. This is a gap in the plugin codebase.

### 2. Dependency Constraints
Follow the dependency versions specified in each `package.json`. Do not upgrade major versions without testing:
- `opencode`: `@opencode-ai/plugin ^1.0.162`, `@opentui/core ^0.1.92`, `solid-js ^1.9.10`
- `mcp`: `@modelcontextprotocol/sdk ^1.0.0`, `zod ^3.23.0`

### 3. ESM Import Extensions (opencode)
OpenCode plugin uses pure ESM. All relative imports **must include `.js` extension** even for `.ts` source files:
```typescript
// Correct
import { client } from './client.js';

// Incorrect
import { client } from './client';
```

### 4. Claude Code Runtime Dependencies
The Claude Code plugin requires `bash`, `curl`, and `python3` to be available in the shell environment. These are not declared as npm dependencies and will fail silently if missing.

### 5. MCP Transport Limitation
The MCP server uses **stdio transport**, which means it runs as a spawned subprocess. It cannot be used as a long-running HTTP server. This is an MCP protocol constraint, not a bug.

### 6. Plugin Version Drift
Plugin versions are managed independently from the server version:
- opencode: `1.8.0`
- openclaw: `0.3.2`
- mcp: `0.3.0`

Ensure API compatibility when updating the server — breaking REST API changes require coordinated plugin updates.

---

*License: Apache-2.0*
