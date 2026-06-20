#!/usr/bin/env node
// SessionStart hook — the CORE feature.
// Injects user preferences (profile) + project memories + global memories at session start.
// Ported from plugins/opencode/src/hooks.ts buildMemoryInjection (L240-320) + chatMessageRecallHook (L324-412).
//
// Input (zcode/claude-code convention, from stdin):
//   { session_id, cwd, transcript_path, hook_event_name, source }
// Output (stdout):
//   { "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": "..." } }
//
// Design: per-message recall is DEPRECATED in opencode. We only inject ONCE at session start.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { loadConfig } from "./lib/config.js";
import { CerebroClient } from "./lib/cerebro-client.js";
import { logInfo, logError, logDebug, logWarn } from "./lib/logger.js";
import {
  getUserTag,
  getProjectTag,
  detectProjectName,
  extractUserRequest,
  formatRelativeAge,
  truncateAtBoundary,
} from "./lib/util.js";

// Read hook input from stdin (zcode pipes JSON context)
function readStdin() {
  return new Promise((resolve) => {
    let data = "";
    if (process.stdin.isTTY) return resolve({});
    process.stdin.setEncoding("utf-8");
    process.stdin.on("data", (chunk) => (data += chunk));
    process.stdin.on("end", () => {
      try {
        resolve(data.trim() ? JSON.parse(data) : {});
      } catch (e) {
        logWarn("readStdin JSON parse failed", { len: data.length, preview: data.slice(0, 120), error: String(e) });
        resolve({});
      }
    });
    process.stdin.on("error", () => resolve({}));
    // Safety timeout — never block session start
    setTimeout(() => resolve({}), 2000);
  });
}

// Emit context injection. zcode (Claude Code-compatible) reads
// hookSpecificOutput.additionalContext. Also emit additionalContext (top-level)
// for SDK/Copilot-style hosts. Field names differ; both are safe to emit.
function emitContext(text) {
  const out = {
    hookSpecificOutput: {
      hookEventName: "SessionStart",
      additionalContext: text,
    },
  };
  process.stdout.write(JSON.stringify(out));
}

function emitEmpty() {
  process.stdout.write("{}");
}

// Build the [CEREBRO-MEMORY] block — port of buildMemoryInjection (hooks.ts:240-320)
async function buildInjection(client, projectPath, query, config) {
  const maxChars = config.content?.maxContentChars ?? 30000;
  const ic = config.injection ?? {};
  const recentCount = ic.recentCount || 5;
  const searchCount = ic.searchCount || 10;
  const recentTruncate = ic.recentTruncateChars || 0; // 0 = no truncation
  const searchTruncate = ic.searchTruncateChars || 0;

  const withTimeout = (p, ms, fallback) =>
    Promise.race([
      p.catch(() => fallback),
      new Promise((resolve) => setTimeout(() => resolve(fallback), ms)),
    ]);

  // Three concurrent fetches with degraded timeouts — must never block session
  const [profile, projectMemories, searchResults] = await Promise.all([
    withTimeout(client.getInjection(projectPath), 1000, null),
    withTimeout(client.listRecent(recentCount, projectPath), 1000, []),
    query
      ? withTimeout(client.searchMemories(query, searchCount, undefined, undefined, projectPath), 1500, [])
      : Promise.resolve([]),
  ]);

  const sections = ["[CEREBRO-MEMORY]", ""];

  // 1. User preferences / profile
  if (profile?.content) {
    sections.push(profile.content);
    sections.push("");
  }

  const seenIds = new Set();

  // 2. Recent project memories
  if (projectMemories && projectMemories.length > 0) {
    sections.push("## Recent Project Activity");
    for (const m of projectMemories) {
      seenIds.add(m.id);
      const age = formatRelativeAge(m.updated_at || m.created_at);
      const content = recentTruncate > 0 ? truncateAtBoundary(m.content, recentTruncate) : m.content;
      sections.push(`- (${age}) ${content}`);
    }
    sections.push("");
  }

  // 3. Relevant memories (semantic search), deduped against project recent
  const deduped = (searchResults || []).filter((r) => !seenIds.has(r.memory?.id));
  if (deduped.length > 0) {
    sections.push("## Relevant Memories");
    for (const r of deduped) {
      const age = formatRelativeAge(r.memory?.created_at);
      const content =
        searchTruncate > 0 ? truncateAtBoundary(r.memory?.content, searchTruncate) : r.memory?.content;
      sections.push(`- (${age}) ${content}`);
    }
    sections.push("");
  }

  sections.push("[/CEREBRO-MEMORY]");

  let text = sections.join("\n");
  if (text.length > maxChars) {
    const cutoff = text.lastIndexOf("\n", maxChars);
    text = text.slice(0, cutoff > 0 ? cutoff : maxChars) + "\n…\n[/CEREBRO-MEMORY]";
  }

  const maxScore = (searchResults || []).reduce((max, r) => Math.max(max, r.score || 0), 0);
  return {
    text,
    profileCount: profile?.preference_count ?? 0,
    projectMemoryCount: projectMemories?.length ?? 0,
    memoryCount: deduped.length,
    maxScore,
  };
}

// Try to derive a query from the first user message in the transcript.
// zcode SessionStart fires BEFORE the first user turn, so transcript may be
// empty (cold start) — that's fine, we degrade to project-name query.
async function deriveQuery(input, projectPath) {
  const transcriptPath = input?.transcript_path;
  if (transcriptPath) {
    try {
      const raw = readFileSync(transcriptPath, "utf-8");
      // JSONL or JSON array
      const lines = raw.split(/\r?\n/).filter(Boolean);
      for (const line of lines) {
        try {
          const entry = JSON.parse(line);
          const role = entry.role || entry.info?.role;
          if (role !== "user") continue;
          let content = entry.content;
          if (Array.isArray(content)) {
            content = content
              .filter((p) => p?.type === "text")
              .map((p) => p.text || "")
              .join("\n");
          }
          if (typeof content !== "string") continue;
          const cleaned = extractUserRequest(content);
          if (cleaned && !/^(hi|hello|hey|你好|嗨|嗯|ok|okay|好的|收到|\s*)$/i.test(cleaned.trim())) {
            return cleaned.slice(0, 500);
          }
        } catch {}
      }
    } catch {}
  }

  // Fallback: project name as query
  if (projectPath) {
    const name = await detectProjectName(projectPath);
    if (name) return name;
  }
  return "";
}

// Auto-start the web UI as a detached daemon if not already running.
// Probe /health first; only spawn if no server responds. The daemon outlives
// the hook process (detached, unref'd) so it stays up for the whole ZCode session.
function ensureWebServer(config) {
  if (config.web?.enabled === false) return;
  const port = config.web?.port || 5212;

  fetch(`http://127.0.0.1:${port}/health`)
    .then((r) => r.json())
    .then((body) => {
      if (body?.service === "cerebro") {
        logDebug("web-server already running", { port });
      } else {
        spawnWebServer(port);
      }
    })
    .catch(() => spawnWebServer(port));
}

function spawnWebServer(port) {
  try {
    const webServerPath = join(dirname(fileURLToPath(import.meta.url)), "..", "web-server.js");
    const child = spawn(process.execPath, [webServerPath], {
      detached: true,
      stdio: "ignore",
      env: { ...process.env, OMEM_LOCAL_PORT: String(port) },
    });
    child.unref();
    logInfo("web-server daemon spawned", { port, pid: child.pid });
  } catch (err) {
    logError("web-server spawn failed", { error: String(err) });
  }
}

async function main() {
  const input = await readStdin();
  const config = loadConfig();

  if (!config.connection.apiKey) {
    const msg =
      "[cerebro] OMEM_API_KEY not set — memory injection disabled.\n\n" +
      "To enable persistent memory, set your API key:\n" +
      "  export OMEM_API_KEY=\"your-key\"\n\n" +
      "Or configure in ~/.config/cerebro/config.json.\n" +
      "Then restart the session.";
    emitContext(msg);
    return;
  }

  const client = new CerebroClient(config.connection.apiUrl, config.connection.apiKey, config);

  // Auto-start web UI daemon (fire-and-forget, non-blocking)
  ensureWebServer(config);

  // Health check (fast-fail surface)
  try {
    await client.getStats();
  } catch (err) {
    const errMsg = err instanceof Error ? err.message : String(err);
    logError("SessionStart health check failed", { apiUrl: config.connection.apiUrl, error: errMsg });
    emitContext(
      `[cerebro] Cannot reach ${config.connection.apiUrl}.\nError: ${errMsg.slice(0, 150)}\nMemory injection disabled this session.`,
    );
    return;
  }

  // Resolve project path: prefer cwd from hook input, fallback to CLAUDE_PROJECT_DIR env
  const projectPath = input?.cwd || process.env.CLAUDE_PROJECT_DIR || process.env.OMEM_PROJECT_DIR || "";
  const query = await deriveQuery(input, projectPath);

  logInfo("SessionStart building injection", {
    projectPath: projectPath || "(none)",
    query: query ? query.slice(0, 80) : "(none)",
  });

  try {
    const injection = await buildInjection(client, projectPath, query, config);
    const hasContent =
      injection.profileCount > 0 || injection.memoryCount > 0 || injection.projectMemoryCount > 0;

    if (injection.text && hasContent && injection.text.length > 20) {
      logInfo("SessionStart injection emitted", {
        profileCount: injection.profileCount,
        projectMemoryCount: injection.projectMemoryCount,
        memoryCount: injection.memoryCount,
        maxScore: injection.maxScore.toFixed(3),
        textLen: injection.text.length,
      });
      emitContext(injection.text);

      // Fire-and-forget recall event (port of hooks.ts:384-397)
      const containerTags = [getUserTag(process.env.GIT_AUTHOR_EMAIL), getProjectTag(projectPath)];
      client
        .createRecallEvent({
          session_id: input?.sessionId || input?.session_id,
          recall_type: "auto",
          query_text: query,
          max_score: injection.maxScore,
          llm_confidence: Math.min(injection.maxScore, 1.0),
          profile_injected: injection.profileCount > 0,
          kept_count: injection.projectMemoryCount + injection.memoryCount,
          discarded_count: 0,
          injected_count: injection.projectMemoryCount + injection.memoryCount,
          injected_content: injection.text,
        })
        .catch((e) => logError("createRecallEvent failed", { error: String(e) }));
    } else {
      logDebug("SessionStart: no content available to inject", {
        profileCount: injection.profileCount,
        projectMemoryCount: injection.projectMemoryCount,
        memoryCount: injection.memoryCount,
      });
      emitEmpty();
    }
  } catch (err) {
    logError("SessionStart injection failed", { error: String(err) });
    emitEmpty();
  }
}

main().catch((err) => {
  logError("SessionStart fatal", { error: String(err) });
  emitEmpty();
});
