#!/usr/bin/env node
// Enhanced Cerebro MCP server for ZCode — 17 tools (aligned with opencode plugin).
// Extends @ourmem/mcp (15 tools) with: memory_profile_stats, memory_toggle,
// and project_path support on memory_search.
//
// Usage (standalone, requires @modelcontextprotocol/sdk + zod installed):
//   node mcp/server.js
//
// Or via .mcp.json:
//   { "mcpServers": { "cerebro": { "command": "node", "args": ["${CLAUDE_PLUGIN_ROOT}/mcp/server.js"] } } }
//
// For a zero-dependency default, use the published package instead:
//   { "mcpServers": { "cerebro": { "command": "npx", "args": ["-y", "@ourmem/mcp"] } } }

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { readFileSync, writeFileSync, mkdirSync, appendFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// ── Startup probe (diagnostic: confirm zcode actually spawns this process) ──
try {
  const probeLog = join(homedir(), ".config", "cerebro", "logs", "mcp-spawn-probe.log");
  mkdirSync(join(homedir(), ".config", "cerebro", "logs"), { recursive: true });
  appendFileSync(
    probeLog,
    `[${new Date().toISOString()}] mcp/server.js SPAWNED | cwd=${process.cwd()} | CLAUDE_PLUGIN_ROOT=${process.env.CLAUDE_PLUGIN_ROOT || "(unset)"} | argv=${JSON.stringify(process.argv)}\n`,
  );
} catch {}
import { createHash } from "node:crypto";

// ── Config ────────────────────────────────────────────────────────────
// Load from ~/.config/cerebro/config.json first (shared with hooks), then
// fall back to env vars. This avoids depending on MCP env-var substitution
// (zcode only substitutes CLAUDE_PLUGIN_ROOT / ZCODE_* / user_config.*).
function loadCerebroConfig() {
  const cfgPath = join(homedir(), ".config", "cerebro", "config.json");
  try {
    const raw = JSON.parse(readFileSync(cfgPath, "utf-8"));
    return {
      apiUrl: raw?.connection?.apiUrl,
      apiKey: raw?.connection?.apiKey,
    };
  } catch {
    return {};
  }
}
const CEREBRO_CFG = loadCerebroConfig();
const API_URL = (process.env.OMEM_API_URL || CEREBRO_CFG.apiUrl || "https://www.mengxy.cc").replace(/\/+$/, "");
const API_KEY = process.env.OMEM_API_KEY || CEREBRO_CFG.apiKey || "";
const AGENT_ID = process.env.OMEM_AGENT_ID || "zcode";
const PROJECT_PATH = process.env.CLAUDE_PROJECT_DIR || process.env.OMEM_PROJECT_DIR || "";
const MAX_QUERY_LENGTH = 200;
const MAX_CONTENT_CHARS = 30000;
const STATE_DIR = process.env.ZCODE_PLUGIN_DATA || join(homedir(), ".config", "cerebro", "zcode-state");

function shortError(prefix, err) {
  const msg = err instanceof Error ? err.message : String(err);
  return `${prefix}: ${msg.slice(0, 200)}`;
}

function sanitizeContent(text, maxLen = MAX_CONTENT_CHARS) {
  if (!text) return "";
  let clean = text.replace(/<[\w-]+[^>]*>[\s\S]*?<\/[\w-]+>/g, "");
  clean = clean.replace(/<[\w-]+[^>]*\/>/g, "");
  clean = clean.replace(/\s+/g, " ").trim();
  if (clean.length <= maxLen) return clean;
  return clean.slice(0, maxLen) + "…[truncated]";
}

// ── HTTP client ───────────────────────────────────────────────────────
async function request(path, init = {}, timeoutMs = 15000) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(`${API_URL}${path}`, {
      ...init,
      signal: controller.signal,
      headers: {
        "Content-Type": "application/json",
        "X-API-Key": API_KEY,
        ...(init.headers || {}),
      },
    });
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`[cerebro] ${res.status} ${res.statusText}${body ? ": " + body.slice(0, 200) : ""}`);
    }
    if (res.status === 204) return null;
    const text = await res.text();
    const trimmed = text.replace(/^\uFEFF/, "").trim();
    if (!trimmed) return null;
    return JSON.parse(trimmed);
  } finally {
    clearTimeout(timeout);
  }
}

const post = (path, body, timeoutMs) =>
  request(path, { method: "POST", body: JSON.stringify(body) }, timeoutMs);
const put = (path, body) => request(path, { method: "PUT", body: JSON.stringify(body) });
const del = (path) => request(path, { method: "DELETE" });

function getUserTag() {
  const id = process.env.GIT_AUTHOR_EMAIL || process.env.USER || process.env.USERNAME || "unknown";
  return `omem_user_${createHash("sha256").update(id).digest("hex").slice(0, 16)}`;
}
function getProjectTag() {
  return `omem_project_${createHash("sha256").update(PROJECT_PATH || process.cwd()).digest("hex").slice(0, 16)}`;
}

// ── Auto-store toggle (per-session, persisted) ────────────────────────
function autoStorePath(sessionId) {
  return join(STATE_DIR, `autostore-${sessionId || "default"}.json`);
}
function getAutoStore(sessionId) {
  try {
    const data = JSON.parse(readFileSync(autoStorePath(sessionId), "utf-8"));
    return data.enabled ?? true;
  } catch {
    return true;
  }
}
function setAutoStore(sessionId, enabled) {
  try {
    mkdirSync(STATE_DIR, { recursive: true });
    writeFileSync(autoStorePath(sessionId), JSON.stringify({ enabled, ts: Date.now() }));
  } catch {}
}

// ── Server setup ──────────────────────────────────────────────────────
const server = new McpServer({ name: "cerebro-zcode", version: "0.1.0" });

if (!API_KEY) {
  console.error("[cerebro-zcode-mcp] OMEM_API_KEY not set — tools will error on call.");
}

function textResult(text, isError = false) {
  return { content: [{ type: "text", text }], isError };
}

const containerTags = [getUserTag(), getProjectTag()];

// ── Tool: memory_store ────────────────────────────────────────────────
server.registerTool(
  "memory_store",
  {
    title: "Store Memory",
    description:
      "Store a new memory. Use for important info, decisions, preferences, or context. " +
      "Categorize: cases (debugging/experience), preferences, entities (people/projects), events, profile, patterns. " +
      "Use scope=project for project-specific, scope=global for cross-project. " +
      "Use visibility=private for passwords/API keys/personal data (isolated by agent_id).",
    inputSchema: {
      content: z.string().describe("The content to remember"),
      tags: z.array(z.string()).optional().describe("Tags to categorize"),
      source: z.string().optional().describe("Source identifier"),
      scope: z.enum(["project", "global"]).optional().describe("project=project-specific, global=cross-project"),
      visibility: z.enum(["global", "private"]).optional().describe("private=isolate sensitive data"),
      category: z
        .enum(["cases", "preferences", "entities", "events", "profile", "patterns"])
        .optional(),
    },
  },
  async ({ content, tags, source, scope, visibility, category }) => {
    try {
      const mem = await post("/v1/memories", {
        content: sanitizeContent(content),
        tags,
        source,
        scope,
        visibility,
        category,
        agent_id: AGENT_ID,
        project_path: PROJECT_PATH || undefined,
      });
      return textResult(`Memory stored (id: ${mem.id}):\n${mem.content}`);
    } catch (err) {
      return textResult(shortError("Failed to store memory", err), true);
    }
  },
);

// ── Tool: memory_search (enhanced with project_path) ──────────────────
server.registerTool(
  "memory_search",
  {
    title: "Search Memories",
    description:
      "Semantic search across stored memories. Returns ranked results by similarity.",
    inputSchema: {
      query: z.string().describe("Search query"),
      limit: z.number().int().min(1).max(50).optional().describe("Max results (default 10)"),
      scope: z.string().optional().describe("Scope filter"),
      tags: z.array(z.string()).optional().describe("Filter by tags"),
    },
  },
  async ({ query, limit, scope, tags }) => {
    try {
      const safeQ = (query || "").slice(0, MAX_QUERY_LENGTH);
      const params = new URLSearchParams({ q: safeQ, limit: String(limit ?? 10) });
      if (scope) params.set("scope", scope);
      if (tags && tags.length > 0) params.set("tags", tags.join(","));
      if (PROJECT_PATH) params.set("project_path", PROJECT_PATH);
      const res = await request(`/v1/memories/search?${params}`, {}, 20000);
      const results = res?.results ?? [];
      if (results.length === 0) return textResult("No memories found.");
      const formatted = results
        .map((r, i) => {
          const t = r.memory?.tags?.length ? ` [${r.memory.tags.join(", ")}]` : "";
          return `${i + 1}. (score: ${(r.score || 0).toFixed(2)})${t}\n   ${r.memory?.content ?? ""}`;
        })
        .join("\n\n");
      return textResult(formatted);
    } catch (err) {
      return textResult(shortError("Search failed", err), true);
    }
  },
);

// ── Tool: memory_get ──────────────────────────────────────────────────
server.registerTool(
  "memory_get",
  {
    title: "Get Memory",
    description: "Retrieve a specific memory by its full ID. Use after search to read the complete untruncated content.",
    inputSchema: { id: z.string().describe("The memory ID") },
  },
  async ({ id }) => {
    try {
      const mem = await request(`/v1/memories/${encodeURIComponent(id)}`);
      if (!mem) return textResult(`Memory ${id} not found.`);
      return textResult(JSON.stringify(mem, null, 2));
    } catch (err) {
      return textResult(shortError("Failed to get memory", err), true);
    }
  },
);

// ── Tool: memory_update ───────────────────────────────────────────────
server.registerTool(
  "memory_update",
  {
    title: "Update Memory",
    description: "Update content or tags of an existing memory. Use when info needs correction or enrichment.",
    inputSchema: {
      id: z.string().describe("Memory ID to update"),
      content: z.string().describe("New content"),
      tags: z.array(z.string()).optional().describe("Replacement tags"),
    },
  },
  async ({ id, content, tags }) => {
    try {
      await put(`/v1/memories/${encodeURIComponent(id)}`, { content, tags });
      return textResult(`Memory ${id} updated.`);
    } catch (err) {
      return textResult(shortError("Failed to update memory", err), true);
    }
  },
);

// ── Tool: memory_delete ───────────────────────────────────────────────
server.registerTool(
  "memory_delete",
  {
    title: "Delete Memory",
    description: "Delete a memory by ID. Irreversible.",
    inputSchema: { id: z.string().describe("Memory ID to delete") },
  },
  async ({ id }) => {
    try {
      await del(`/v1/memories/${encodeURIComponent(id)}`);
      return textResult(`Memory ${id} deleted.`);
    } catch (err) {
      return textResult(shortError("Failed to delete memory", err), true);
    }
  },
);

// ── Tool: memory_list ─────────────────────────────────────────────────
server.registerTool(
  "memory_list",
  {
    title: "List Recent Memories",
    description: "List most recent memories. Browse what's remembered without a search query.",
    inputSchema: { limit: z.number().int().min(1).max(100).optional().describe("Max (default 20)") },
  },
  async ({ limit }) => {
    try {
      const params = new URLSearchParams({
        limit: String(limit ?? 20),
        offset: "0",
        sort: "updated_at",
        order: "desc",
      });
      if (PROJECT_PATH) params.set("project_path", PROJECT_PATH);
      const res = await request(`/v1/memories?${params}`);
      const memories = res?.memories ?? [];
      if (memories.length === 0) return textResult("No memories stored yet.");
      const formatted = memories
        .map((m, i) => {
          const t = m.tags?.length ? ` [${m.tags.join(", ")}]` : "";
          return `${i + 1}. (${m.category})${t} ${(m.content ?? "").slice(0, 120)}`;
        })
        .join("\n");
      return textResult(formatted);
    } catch (err) {
      return textResult(shortError("Failed to list memories", err), true);
    }
  },
);

// ── Tool: memory_profile ──────────────────────────────────────────────
server.registerTool(
  "memory_profile",
  {
    title: "User Profile",
    description: "Get synthesized user profile (preferences) from stored memories.",
    inputSchema: {},
  },
  async () => {
    try {
      const params = PROJECT_PATH ? `?project_path=${encodeURIComponent(PROJECT_PATH)}` : "";
      const profile = await request(`/v2/profile${params}`);
      return textResult(JSON.stringify(profile, null, 2));
    } catch (err) {
      return textResult(shortError("Failed to get profile", err), true);
    }
  },
);

// ── Tool: memory_profile_stats (NEW — opencode-exclusive) ─────────────
server.registerTool(
  "memory_profile_stats",
  {
    title: "Profile Statistics",
    description: "Get profile statistics — preference counts by slot, confidence, scope.",
    inputSchema: {},
  },
  async () => {
    try {
      const stats = await request(`/v2/profile/stats`);
      return textResult(JSON.stringify(stats, null, 2));
    } catch (err) {
      return textResult(shortError("Failed to get profile stats", err), true);
    }
  },
);

// ── Tool: memory_ingest ───────────────────────────────────────────────
server.registerTool(
  "memory_ingest",
  {
    title: "Ingest Conversation",
    description:
      "Ingest conversation messages for intelligent extraction. Extracts atomic facts, deduplicates, reconciles with existing memories.",
    inputSchema: {
      messages: z
        .array(z.object({ role: z.string(), content: z.string() }))
        .describe("Conversation messages"),
      mode: z.enum(["smart", "raw"]).optional().describe("smart=LLM extraction (default), raw=as-is"),
      tags: z.array(z.string()).optional(),
    },
  },
  async ({ messages, mode, tags }) => {
    try {
      const result = await post("/v1/memories", {
        messages: messages.map((m) => ({ role: m.role, content: sanitizeContent(m.content) })),
        mode: mode ?? "smart",
        agent_id: AGENT_ID,
        project_path: PROJECT_PATH || undefined,
        tags,
      });
      return textResult(`Ingestion complete: ${JSON.stringify(result)}`);
    } catch (err) {
      return textResult(shortError("Ingestion failed", err), true);
    }
  },
);

// ── Tool: memory_stats ────────────────────────────────────────────────
server.registerTool(
  "memory_stats",
  {
    title: "Memory Statistics",
    description: "Get memory stats — counts by category, type, tier, timeline.",
    inputSchema: {},
  },
  async () => {
    try {
      const stats = await request(`/v1/stats`);
      return textResult(JSON.stringify(stats, null, 2));
    } catch (err) {
      return textResult(shortError("Failed to get stats", err), true);
    }
  },
);

// ── Tool: memory_toggle (NEW — session-level auto-store control) ──────
server.registerTool(
  "memory_toggle",
  {
    title: "Toggle Auto-Store",
    description:
      "Toggle session-level auto-store (whether Stop/PreCompact hooks archive this session). " +
      "state=on enables, off disables, omit to query current state.",
    inputSchema: {
      state: z.enum(["on", "off"]).optional().describe("on/off, or omit to query"),
      session_id: z.string().optional().describe("Session ID (defaults to 'default')"),
    },
  },
  async ({ state, session_id }) => {
    const sid = session_id || "default";
    if (!state) {
      const current = getAutoStore(sid);
      return textResult(`Auto-store for session ${sid}: ${current ? "ON" : "OFF"}`);
    }
    const enabled = state === "on";
    setAutoStore(sid, enabled);
    return textResult(`Auto-store for session ${sid} set to ${enabled ? "ON" : "OFF"}`);
  },
);

// ── Tool: space_create ────────────────────────────────────────────────
server.registerTool(
  "space_create",
  {
    title: "Create Space",
    description: "Create a shared space (team/organization) for sharing memories across users/agents.",
    inputSchema: {
      name: z.string(),
      space_type: z.enum(["team", "organization"]),
      members: z
        .array(z.object({ user_id: z.string(), role: z.enum(["admin", "member", "reader"]) }))
        .optional(),
    },
  },
  async ({ name, space_type, members }) => {
    try {
      const space = await post("/v1/spaces", { name, space_type, members });
      return textResult(`Space created:\n${JSON.stringify(space, null, 2)}`);
    } catch (err) {
      return textResult(shortError("Failed to create space", err), true);
    }
  },
);

// ── Tool: space_list ──────────────────────────────────────────────────
server.registerTool(
  "space_list",
  {
    title: "List Spaces",
    description: "List all spaces you own or are a member of.",
    inputSchema: {},
  },
  async () => {
    try {
      const res = await request(`/v1/spaces`);
      const spaces = res?.spaces ?? [];
      if (spaces.length === 0) return textResult("No spaces found.");
      return textResult(JSON.stringify(spaces, null, 2));
    } catch (err) {
      return textResult(shortError("Failed to list spaces", err), true);
    }
  },
);

// ── Tool: space_add_member ────────────────────────────────────────────
server.registerTool(
  "space_add_member",
  {
    title: "Add Space Member",
    description: "Add a user to an existing shared space with a specified role.",
    inputSchema: {
      space_id: z.string(),
      user_id: z.string(),
      role: z.enum(["admin", "member", "reader"]),
    },
  },
  async ({ space_id, user_id, role }) => {
    try {
      const result = await post(`/v1/spaces/${encodeURIComponent(space_id)}/members`, { user_id, role });
      return textResult(`Member added:\n${JSON.stringify(result, null, 2)}`);
    } catch (err) {
      return textResult(shortError("Failed to add member", err), true);
    }
  },
);

// ── Tool: memory_share ────────────────────────────────────────────────
server.registerTool(
  "memory_share",
  {
    title: "Share Memory",
    description: "Share a memory to a team/organization space with full provenance + vector embedding.",
    inputSchema: { memory_id: z.string(), target_space: z.string() },
  },
  async ({ memory_id, target_space }) => {
    try {
      const result = await post(`/v1/memories/${encodeURIComponent(memory_id)}/share`, { target_space });
      return textResult(`Memory shared:\n${JSON.stringify(result, null, 2)}`);
    } catch (err) {
      return textResult(shortError("Failed to share memory", err), true);
    }
  },
);

// ── Tool: memory_pull ─────────────────────────────────────────────────
server.registerTool(
  "memory_pull",
  {
    title: "Pull Memory",
    description: "Pull a shared memory from a team/organization space into your personal space.",
    inputSchema: {
      memory_id: z.string(),
      source_space: z.string(),
      visibility: z.string().optional(),
    },
  },
  async ({ memory_id, source_space, visibility }) => {
    try {
      const result = await post(`/v1/memories/${encodeURIComponent(memory_id)}/pull`, {
        source_space,
        visibility,
      });
      return textResult(`Memory pulled:\n${JSON.stringify(result, null, 2)}`);
    } catch (err) {
      return textResult(shortError("Failed to pull memory", err), true);
    }
  },
);

// ── Tool: memory_reshare ──────────────────────────────────────────────
server.registerTool(
  "memory_reshare",
  {
    title: "Reshare Memory",
    description: "Refresh a stale shared copy with the latest content and vector from source.",
    inputSchema: { memory_id: z.string(), target_space: z.string().optional() },
  },
  async ({ memory_id, target_space }) => {
    try {
      const result = await post(`/v1/memories/${encodeURIComponent(memory_id)}/reshare`, { target_space });
      return textResult(`Memory reshared:\n${JSON.stringify(result, null, 2)}`);
    } catch (err) {
      return textResult(shortError("Failed to reshare memory", err), true);
    }
  },
);

// ── Bootstrap ─────────────────────────────────────────────────────────
async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error(`[cerebro-zcode-mcp] Server running on stdio (API: ${API_URL})`);
}

main().catch((err) => {
  console.error("[cerebro-zcode-mcp] Fatal:", err);
  process.exit(1);
});
