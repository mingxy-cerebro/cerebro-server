#!/usr/bin/env node
// Cerebro MCP server for ZCode — 17 tools, ZERO npm dependencies.
//
// v0.3.0 rewrite: the @modelcontextprotocol/sdk + zod dependencies are gone.
// The stdio JSON-RPC 2.0 transport is implemented inline (readline + JSON
// lines), so a marketplace / git / directory install works without any
// `npm install` step — hooks AND MCP tools run straight from the plugin dir.
//
// Business logic aligned with the opencode plugin tools.ts and the
// claude-code skills (container tags, enum validation, truncation limits,
// source/agent_id identity, auto-store toggle):
//   - every write auto-prepends container tags (omem_user_*/omem_project_*)
//   - memory_search auto-scopes by container tags + project_path (user isolation)
//   - query capped at 200 chars (HTTP 414 guard), content sanitized at 3000
//   - category/visibility/scope/mode/space_type/role are enum-checked
//   - source defaults to "zcode", agent_id defaults to "zcode"
//
// Usage:
//   node mcp/server.js
// Config: OMEM_API_URL / OMEM_API_KEY env vars > ~/.config/cerebro/config.json

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { createHash } from "node:crypto";
import { execSync } from "node:child_process";
import { detectProjectRoot, toRemoteProjectPath, getAutoStore, setAutoStore } from "../hooks/lib/util.js";

// ── Version (single source: package.json) ─────────────────────────────
const PLUGIN_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
let VERSION = "0.0.0";
try {
  VERSION = JSON.parse(readFileSync(join(PLUGIN_ROOT, "package.json"), "utf-8")).version || "0.0.0";
} catch {}

// ── Config ────────────────────────────────────────────────────────────
function loadCerebroConfig() {
  const cfgPath = join(homedir(), ".config", "cerebro", "config.json");
  try {
    const raw = JSON.parse(readFileSync(cfgPath, "utf-8"));
    return { apiUrl: raw?.connection?.apiUrl, apiKey: raw?.connection?.apiKey };
  } catch {
    return {};
  }
}
const CEREBRO_CFG = loadCerebroConfig();
const API_URL = (process.env.OMEM_API_URL || CEREBRO_CFG.apiUrl || "https://www.mengxy.cc").replace(/\/+$/, "");
const API_KEY = process.env.OMEM_API_KEY || CEREBRO_CFG.apiKey || "";
const AGENT_ID = process.env.OMEM_AGENT_ID || "zcode";
const MAX_QUERY_LENGTH = 200;
const MAX_CONTENT_LENGTH = 3000;

function log(...args) {
  console.error("[cerebro-zcode-mcp]", ...args);
}

// ── HTTP client (fetch, AbortController timeout) ──────────────────────
async function request(path, init = {}, timeoutMs = 15000) {
  if (!API_KEY) throw new Error("OMEM_API_KEY not set — configure ~/.config/cerebro/config.json or the env var");
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

const post = (path, body, timeoutMs) => request(path, { method: "POST", body: JSON.stringify(body) }, timeoutMs);
const put = (path, body) => request(path, { method: "PUT", body: JSON.stringify(body) });
const del = (path) => request(path, { method: "DELETE" });

function shortError(prefix, err) {
  const msg = err instanceof Error ? err.message : String(err);
  return `${prefix}: ${msg.slice(0, 200)}`;
}

function sanitizeContent(text, maxLen = MAX_CONTENT_LENGTH) {
  if (!text) return "";
  let clean = text.replace(/<[\w-]+[^>]*>[\s\S]*?<\/[\w-]+>/g, "");
  clean = clean.replace(/<[\w-]+[^>]*\/>/g, "");
  clean = clean.replace(/\s+/g, " ").trim();
  if (clean.length <= maxLen) return clean;
  return clean.slice(0, maxLen) + "…[truncated]";
}

// ── Container tags (user + project isolation, aligned with opencode/CC) ─
function sha256_16(input) {
  return createHash("sha256").update(input).digest("hex").slice(0, 16);
}

function detectUserEmail() {
  // Canonical identity chain aligned with claude-code containerTags():
  // OMEM_USER_EMAIL > git config user.email > GIT_AUTHOR_EMAIL > USER.
  // Matching this chain across plugins is what keeps the omem_user_<hash>
  // tag — and user-scoped memory isolation — identical across WSL/Windows.
  if (process.env.OMEM_USER_EMAIL) return process.env.OMEM_USER_EMAIL;
  try {
    const email = execSync("git config user.email", {
      encoding: "utf-8",
      timeout: 2000,
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
    if (email) return email;
  } catch {}
  return process.env.GIT_AUTHOR_EMAIL || "";
}

function containerTags(projectPath) {
  const tags = [];
  const email = detectUserEmail();
  if (email) tags.push(`omem_user_${sha256_16(email)}`);
  const dir = projectPath || PROJECT_PATH;
  if (dir) tags.push(`omem_project_${sha256_16(dir)}`);
  return tags;
}

// Effective project path: explicit tool arg > env > git root of cwd.
// Always in canonical (/mnt/<drive>/... on Windows) form — the server-facing
// identity must match what WSL-side plugins record for the same repo.
const PROJECT_PATH = toRemoteProjectPath(
  detectProjectRoot(process.env.CLAUDE_PROJECT_DIR || process.env.OMEM_PROJECT_DIR || process.cwd()),
);

function effectiveProjectPath(args) {
  const p = typeof args?.project_path === "string" && args.project_path.trim() ? args.project_path.trim() : "";
  return p ? toRemoteProjectPath(detectProjectRoot(p)) : PROJECT_PATH;
}

// ── Validation helpers (manual enum/bounds checks, zod-free) ───────────
function bad(msg) {
  return { __error: msg };
}
function checkEnum(value, allowed, name) {
  if (value === undefined || value === null) return {};
  if (typeof value !== "string" || !allowed.includes(value)) {
    return bad(`${name} must be one of: ${allowed.join(", ")}`);
  }
  return { [name]: value };
}
function checkInt(value, name, min, max, dflt) {
  if (value === undefined || value === null) return { [name]: dflt };
  if (!Number.isInteger(value) || value < min || value > max) {
    return bad(`${name} must be an integer between ${min} and ${max}`);
  }
  return { [name]: value };
}
function checkStringArray(value, name) {
  if (value === undefined || value === null) return { [name]: undefined };
  if (!Array.isArray(value) || value.some((x) => typeof x !== "string")) {
    return bad(`${name} must be an array of strings`);
  }
  return { [name]: value };
}

const CATEGORIES = ["cases", "preferences", "entities", "events", "profile", "patterns"];

// ── Tool result helpers ───────────────────────────────────────────────
function text(text_, isError = false) {
  return { content: [{ type: "text", text: text_ }], isError };
}

// ── Tool implementations ──────────────────────────────────────────────
const tools = {};

// Each entry: { title, description, inputSchema (JSON Schema), run(args) }

tools.memory_store = {
  title: "Store Memory",
  description:
    "Store a new memory. Use for important info, decisions, preferences, or context. " +
    "Categorize: cases (debugging/experience), preferences, entities (people/projects), events, profile, patterns. " +
    "Use scope=project for project-specific, scope=global for cross-project. " +
    "Use visibility=private for passwords/API keys/personal data (isolated by agent_id).",
  inputSchema: {
    type: "object",
    properties: {
      content: { type: "string", description: "The content to remember" },
      tags: { type: "array", items: { type: "string" }, description: "Tags to categorize" },
      source: { type: "string", description: "Source identifier (defaults to zcode)" },
      scope: { type: "string", enum: ["project", "global"], description: "project=project-specific, global=cross-project" },
      visibility: { type: "string", enum: ["global", "private"], description: "private=isolate sensitive data" },
      category: { type: "string", enum: CATEGORIES },
      project_path: { type: "string", description: "Project path override (defaults to git root of cwd)" },
    },
    required: ["content"],
  },
  async run(args) {
    const v = {
      ...checkEnum(args.scope, ["project", "global"], "scope"),
      ...checkEnum(args.visibility, ["global", "private"], "visibility"),
      ...checkEnum(args.category, CATEGORIES, "category"),
      ...checkStringArray(args.tags, "tags"),
    };
    if (v.__error) return text(v.__error, true);
    const pp = effectiveProjectPath(args);
    // Container tags auto-prepended on every write (user + project isolation)
    const tags = [...containerTags(pp), ...(v.tags || [])];
    const mem = await post("/v1/memories", {
      content: sanitizeContent(args.content),
      tags,
      source: args.source || "zcode",
      scope: v.scope,
      visibility: v.visibility,
      category: v.category,
      agent_id: AGENT_ID,
      project_path: pp || undefined,
    });
    return text(`Memory stored (id: ${mem.id}):\n${mem.content}`);
  },
};

tools.memory_search = {
  title: "Search Memories",
  description: "Semantic search across stored memories. Returns ranked results by similarity.",
  inputSchema: {
    type: "object",
    properties: {
      query: { type: "string", description: "Search query" },
      limit: { type: "integer", minimum: 1, maximum: 50, description: "Max results (default 10)" },
      scope: { type: "string", enum: ["project", "global"], description: "Scope filter" },
      tags: { type: "array", items: { type: "string" }, description: "Filter by tags" },
      project_path: { type: "string", description: "Project path override" },
    },
    required: ["query"],
  },
  async run(args) {
    const v = {
      ...checkEnum(args.scope, ["project", "global"], "scope"),
      ...checkStringArray(args.tags, "tags"),
      ...checkInt(args.limit, "limit", 1, 50, 10),
    };
    if (v.__error) return text(v.__error, true);
    const pp = effectiveProjectPath(args);
    const safeQ = (args.query || "").slice(0, MAX_QUERY_LENGTH);
    const params = new URLSearchParams({ q: safeQ, limit: String(v.limit) });
    if (v.scope) params.set("scope", v.scope);
    // Auto-scope by container tags unless the caller filtered explicitly
    // (opencode semantics: user + project isolation on search)
    const autoTags = containerTags(pp);
    const allTags = [...autoTags, ...(v.tags || [])];
    if (allTags.length > 0) params.set("tags", allTags.join(","));
    if (pp) params.set("project_path", pp);
    const res = await request(`/v1/memories/search?${params}`, {}, 20000);
    const results = res?.results ?? [];
    if (results.length === 0) return text("No memories found.");
    const formatted = results
      .map((r, i) => {
        const t = r.memory?.tags?.length ? ` [${r.memory.tags.join(", ")}]` : "";
        return `${i + 1}. (score: ${(r.score || 0).toFixed(2)})${t}\n   ${r.memory?.content ?? ""}`;
      })
      .join("\n\n");
    return text(formatted);
  },
};

tools.memory_get = {
  title: "Get Memory",
  description: "Retrieve a specific memory by its full ID. Use after search to read the complete untruncated content.",
  inputSchema: {
    type: "object",
    properties: { id: { type: "string", description: "The memory ID" } },
    required: ["id"],
  },
  async run(args) {
    const mem = await request(`/v1/memories/${encodeURIComponent(args.id)}`);
    if (!mem) return text(`Memory ${args.id} not found.`);
    return text(JSON.stringify(mem, null, 2));
  },
};

tools.memory_update = {
  title: "Update Memory",
  description: "Update content or tags of an existing memory. Use when info needs correction or enrichment.",
  inputSchema: {
    type: "object",
    properties: {
      id: { type: "string", description: "Memory ID to update" },
      content: { type: "string", description: "New content" },
      tags: { type: "array", items: { type: "string" }, description: "Replacement tags" },
    },
    required: ["id", "content"],
  },
  async run(args) {
    const v = checkStringArray(args.tags, "tags");
    if (v.__error) return text(v.__error, true);
    await put(`/v1/memories/${encodeURIComponent(args.id)}`, {
      content: sanitizeContent(args.content),
      tags: v.tags,
    });
    return text(`Memory ${args.id} updated.`);
  },
};

tools.memory_delete = {
  title: "Delete Memory",
  description: "Delete a memory by ID. Irreversible.",
  inputSchema: {
    type: "object",
    properties: { id: { type: "string", description: "Memory ID to delete" } },
    required: ["id"],
  },
  async run(args) {
    await del(`/v1/memories/${encodeURIComponent(args.id)}`);
    return text(`Memory ${args.id} deleted.`);
  },
};

tools.memory_list = {
  title: "List Recent Memories",
  description: "List most recent memories. Browse what's remembered without a search query.",
  inputSchema: {
    type: "object",
    properties: {
      limit: { type: "integer", minimum: 1, maximum: 100, description: "Max (default 20)" },
      project_path: { type: "string", description: "Project path override" },
    },
  },
  async run(args) {
    const v = checkInt(args.limit, "limit", 1, 100, 20);
    if (v.__error) return text(v.__error, true);
    const pp = effectiveProjectPath(args);
    const params = new URLSearchParams({
      limit: String(v.limit),
      offset: "0",
      sort: "updated_at",
      order: "desc",
    });
    if (pp) params.set("project_path", pp);
    const res = await request(`/v1/memories?${params}`);
    const memories = res?.memories ?? [];
    if (memories.length === 0) return text("No memories stored yet.");
    const formatted = memories
      .map((m, i) => {
        const t = m.tags?.length ? ` [${m.tags.join(", ")}]` : "";
        return `${i + 1}. (${m.category})${t} ${(m.content ?? "").slice(0, 120)}`;
      })
      .join("\n");
    return text(formatted);
  },
};

tools.memory_profile = {
  title: "User Profile",
  description: "Get synthesized user profile (preferences) from stored memories.",
  inputSchema: {
    type: "object",
    properties: { project_path: { type: "string", description: "Project path override" } },
  },
  async run(args) {
    const pp = effectiveProjectPath(args);
    const params = pp ? `?project_path=${encodeURIComponent(pp)}` : "";
    const profile = await request(`/v2/profile${params}`);
    return text(JSON.stringify(profile, null, 2));
  },
};

tools.memory_profile_stats = {
  title: "Profile Statistics",
  description: "Get profile statistics — preference counts by slot, confidence, scope.",
  inputSchema: { type: "object", properties: {} },
  async run() {
    const stats = await request(`/v2/profile/stats`);
    return text(JSON.stringify(stats, null, 2));
  },
};

tools.memory_ingest = {
  title: "Ingest Conversation",
  description:
    "Ingest conversation messages for intelligent extraction. Extracts atomic facts, deduplicates, reconciles with existing memories.",
  inputSchema: {
    type: "object",
    properties: {
      messages: {
        type: "array",
        items: { type: "object", properties: { role: { type: "string" }, content: { type: "string" } }, required: ["role", "content"] },
        description: "Conversation messages",
      },
      mode: { type: "string", enum: ["smart", "raw"], description: "smart=LLM extraction (default), raw=as-is" },
      tags: { type: "array", items: { type: "string" } },
      project_path: { type: "string", description: "Project path override" },
    },
    required: ["messages"],
  },
  async run(args) {
    const v = {
      ...checkEnum(args.mode, ["smart", "raw"], "mode"),
      ...checkStringArray(args.tags, "tags"),
    };
    if (v.__error) return text(v.__error, true);
    if (!Array.isArray(args.messages) || args.messages.length === 0) {
      return text("messages must be a non-empty array of {role, content}", true);
    }
    const pp = effectiveProjectPath(args);
    const result = await post("/v1/memories", {
      messages: args.messages.map((m) => ({ role: m.role, content: sanitizeContent(m.content) })),
      mode: v.mode ?? "smart",
      agent_id: AGENT_ID,
      project_path: pp || undefined,
      tags: [...containerTags(pp), ...(v.tags || [])],
    });
    return text(`Ingestion complete: ${JSON.stringify(result)}`);
  },
};

tools.memory_stats = {
  title: "Memory Statistics",
  description: "Get memory stats — counts by category, type, tier, timeline.",
  inputSchema: { type: "object", properties: {} },
  async run() {
    const stats = await request(`/v1/stats`);
    return text(JSON.stringify(stats, null, 2));
  },
};

tools.memory_toggle = {
  title: "Toggle Auto-Store",
  description:
    "Toggle session-level auto-store (whether the Stop hook archives this session). " +
    "state=on enables, off disables, omit to query current state.",
  inputSchema: {
    type: "object",
    properties: {
      state: { type: "string", enum: ["on", "off"], description: "on/off, or omit to query" },
      session_id: { type: "string", description: "Session ID (defaults to 'default')" },
    },
  },
  async run(args) {
    const v = checkEnum(args.state, ["on", "off"], "state");
    if (v.__error) return text(v.__error, true);
    const sid = args.session_id || "default";
    if (!v.state) {
      const current = getAutoStore(sid);
      return text(`Auto-store for session ${sid}: ${current ? "ON" : "OFF"}`);
    }
    const enabled = v.state === "on";
    setAutoStore(sid, enabled);
    return text(`Auto-store for session ${sid} set to ${enabled ? "ON" : "OFF"}`);
  },
};

tools.space_create = {
  title: "Create Space",
  description: "Create a shared space (team/organization) for sharing memories across users/agents.",
  inputSchema: {
    type: "object",
    properties: {
      name: { type: "string" },
      space_type: { type: "string", enum: ["team", "organization"] },
      members: {
        type: "array",
        items: {
          type: "object",
          properties: {
            user_id: { type: "string" },
            role: { type: "string", enum: ["admin", "member", "reader"] },
          },
          required: ["user_id", "role"],
        },
      },
    },
    required: ["name", "space_type"],
  },
  async run(args) {
    const v = checkEnum(args.space_type, ["team", "organization"], "space_type");
    if (v.__error) return text(v.__error, true);
    const space = await post("/v1/spaces", { name: args.name, space_type: v.space_type, members: args.members });
    return text(`Space created:\n${JSON.stringify(space, null, 2)}`);
  },
};

tools.space_list = {
  title: "List Spaces",
  description: "List all spaces you own or are a member of.",
  inputSchema: { type: "object", properties: {} },
  async run() {
    const res = await request(`/v1/spaces`);
    const spaces = res?.spaces ?? [];
    if (spaces.length === 0) return text("No spaces found.");
    return text(JSON.stringify(spaces, null, 2));
  },
};

tools.space_add_member = {
  title: "Add Space Member",
  description: "Add a user to an existing shared space with a specified role.",
  inputSchema: {
    type: "object",
    properties: {
      space_id: { type: "string" },
      user_id: { type: "string" },
      role: { type: "string", enum: ["admin", "member", "reader"] },
    },
    required: ["space_id", "user_id", "role"],
  },
  async run(args) {
    const v = checkEnum(args.role, ["admin", "member", "reader"], "role");
    if (v.__error) return text(v.__error, true);
    const result = await post(`/v1/spaces/${encodeURIComponent(args.space_id)}/members`, {
      user_id: args.user_id,
      role: v.role,
    });
    return text(`Member added:\n${JSON.stringify(result, null, 2)}`);
  },
};

tools.memory_share = {
  title: "Share Memory",
  description: "Share a memory to a team/organization space with full provenance + vector embedding.",
  inputSchema: {
    type: "object",
    properties: { memory_id: { type: "string" }, target_space: { type: "string" } },
    required: ["memory_id", "target_space"],
  },
  async run(args) {
    const result = await post(`/v1/memories/${encodeURIComponent(args.memory_id)}/share`, {
      target_space: args.target_space,
    });
    return text(`Memory shared:\n${JSON.stringify(result, null, 2)}`);
  },
};

tools.memory_pull = {
  title: "Pull Memory",
  description: "Pull a shared memory from a team/organization space into your personal space.",
  inputSchema: {
    type: "object",
    properties: {
      memory_id: { type: "string" },
      source_space: { type: "string" },
      visibility: { type: "string", enum: ["global", "private"] },
    },
    required: ["memory_id", "source_space"],
  },
  async run(args) {
    const v = checkEnum(args.visibility, ["global", "private"], "visibility");
    if (v.__error) return text(v.__error, true);
    const result = await post(`/v1/memories/${encodeURIComponent(args.memory_id)}/pull`, {
      source_space: args.source_space,
      visibility: v.visibility,
    });
    return text(`Memory pulled:\n${JSON.stringify(result, null, 2)}`);
  },
};

tools.memory_reshare = {
  title: "Reshare Memory",
  description: "Refresh a stale shared copy with the latest content and vector from source.",
  inputSchema: {
    type: "object",
    properties: { memory_id: { type: "string" }, target_space: { type: "string" } },
    required: ["memory_id"],
  },
  async run(args) {
    const result = await post(`/v1/memories/${encodeURIComponent(args.memory_id)}/reshare`, {
      target_space: args.target_space,
    });
    return text(`Memory reshared:\n${JSON.stringify(result, null, 2)}`);
  },
};

// ── JSON-RPC 2.0 stdio transport ──────────────────────────────────────
function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function rpcError(id, code, message) {
  send({ jsonrpc: "2.0", id, error: { code, message } });
}

async function handleMessage(msg) {
  const method = msg.method;
  const id = msg.id ?? null;
  const params = msg.params || {};

  // Notifications (no id) never get a response
  if (id === null) return;

  try {
    switch (method) {
      case "initialize":
        send({
          jsonrpc: "2.0",
          id,
          result: {
            protocolVersion: params.protocolVersion || "2024-11-05",
            capabilities: { tools: {} },
            serverInfo: { name: "cerebro-zcode", version: VERSION },
          },
        });
        break;

      case "ping":
        send({ jsonrpc: "2.0", id, result: {} });
        break;

      case "tools/list":
        send({
          jsonrpc: "2.0",
          id,
          result: {
            tools: Object.entries(tools).map(([name, t]) => ({
              name,
              title: t.title,
              description: t.description,
              inputSchema: t.inputSchema,
            })),
          },
        });
        break;

      case "tools/call": {
        const name = params.name;
        const args = params.arguments || {};
        const tool = tools[name];
        if (!tool) {
          rpcError(id, -32602, `Unknown tool: ${name}`);
          break;
        }
        try {
          const result = await tool.run(args);
          send({ jsonrpc: "2.0", id, result });
        } catch (err) {
          // Tool execution failure → tool result with isError, not RPC error
          send({
            jsonrpc: "2.0",
            id,
            result: { content: [{ type: "text", text: shortError(`Tool ${name} failed`, err) }], isError: true },
          });
        }
        break;
      }

      // Empty implementations so clients probing these don't error out
      case "resources/list":
        send({ jsonrpc: "2.0", id, result: { resources: [] } });
        break;
      case "prompts/list":
        send({ jsonrpc: "2.0", id, result: { prompts: [] } });
        break;

      default:
        rpcError(id, -32601, `Method not found: ${method}`);
    }
  } catch (err) {
    rpcError(id, -32603, `Internal error: ${shortError("", err)}`);
  }
}

async function main() {
  if (!API_KEY) {
    log("OMEM_API_KEY not set — tools will error on call.");
  }
  log(`Server starting on stdio (v${VERSION}, API: ${API_URL})`);

  const rl = createInterface({ input: process.stdin, terminal: false });
  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let msg;
    try {
      msg = JSON.parse(trimmed);
    } catch {
      log("Ignoring non-JSON line");
      return;
    }
    handleMessage(msg).catch((err) => log("handleMessage failed:", String(err)));
  });
  rl.on("close", () => {
    log("stdin closed, exiting");
    process.exit(0);
  });
}

main().catch((err) => {
  log("Fatal:", String(err));
  process.exit(1);
});
