#!/usr/bin/env node
// Stop hook — incremental session archive at each turn end.
// ZCode fires Stop after each assistant response. Unlike Claude Code, ZCode does
// NOT pass transcript_path via stdin. It provides sessionId + turnId (camelCase).
// We read the conversation from ZCode's rollout JSONL:
//   ~/.zcode/cli/rollout/model-io-sess_<sessionId>.jsonl
// Each line = one turn; request.messages is a FULL snapshot of the conversation.
//
// Incremental algorithm (v0.3.0):
//   State: { turnIds: [...processed], lastMsgCount: N }
//   delta  = latestTurn.conversation.slice(lastMsgCount)   — no re-ingest
//   If latestTurn.length <= lastMsgCount (compact reset) → delta = full latest
//   Threshold gates on the TOTAL conversation length (opencode semantics), and
//   below-threshold does NOT advance state — messages accumulate until enough.
//   2xx advances state; failure keeps state for retry (at-least-once).
//
// Cleaning ports claude-code flushSessionIngest: strip inject-echo blocks
// (<system-reminder>, <cerebro-*>, [CEREBRO-MEMORY]), drop thinking, truncate
// tool_result (500) / tool_use (100), drop messages < 100 chars after cleaning.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { loadConfig } from "./lib/config.js";
import { CerebroClient } from "./lib/cerebro-client.js";
import { logInfo, logError, logDebug, logWarn } from "./lib/logger.js";
import {
  detectProjectName,
  detectProjectRoot,
  toRemoteProjectPath,
  cleanText,
  contentText,
  getAutoStore,
} from "./lib/util.js";

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
        logWarn("readStdin JSON parse failed", { len: data.length, error: String(e) });
        resolve({});
      }
    });
    process.stdin.on("error", () => resolve({}));
    setTimeout(() => resolve({}), 2000);
  });
}

function stateDir() {
  if (process.env.ZCODE_PLUGIN_DATA) return process.env.ZCODE_PLUGIN_DATA;
  return join(homedir(), ".config", "cerebro", "zcode-state");
}

function statePath(sessionId) {
  return join(stateDir(), `processed-turns-${sessionId || "default"}.json`);
}

function loadState(sessionId) {
  try {
    const data = JSON.parse(readFileSync(statePath(sessionId), "utf-8"));
    return { turnIds: new Set(Array.isArray(data.turnIds) ? data.turnIds : []), lastMsgCount: data.lastMsgCount || 0 };
  } catch {
    return { turnIds: new Set(), lastMsgCount: 0 };
  }
}

function saveState(sessionId, turnIds, lastMsgCount) {
  try {
    mkdirSync(stateDir(), { recursive: true });
    writeFileSync(statePath(sessionId), JSON.stringify({ turnIds: [...turnIds], lastMsgCount, ts: Date.now() }));
  } catch {}
}

// Read rollout JSONL → [{turnId, conversation:[{role,content}]}]
// conversation = request.messages (full snapshot) + response.text appended.
function readRollout(sessionId) {
  if (!sessionId) return [];
  // zcode rollout files are named model-io-sess_<uuid>.jsonl
  // sessionId may come as "sess_<uuid>" or "<uuid>" — strip leading sess_ prefix
  const sid = sessionId.replace(/^sess_/, "");
  const rolloutPath = join(homedir(), ".zcode", "cli", "rollout", `model-io-sess_${sid}.jsonl`);
  let raw;
  try {
    raw = readFileSync(rolloutPath, "utf-8");
  } catch {
    return [];
  }

  const turns = [];
  for (const line of raw.split(/\r?\n/).filter(Boolean)) {
    try {
      const entry = JSON.parse(line);
      const turnId = entry.turnId;
      if (!turnId) continue;

      const msgs = entry.request?.messages || entry.request?.body?.messages || [];
      if (!Array.isArray(msgs) || msgs.length === 0) continue;

      const conversation = [];
      for (const m of msgs) {
        const role = m.role;
        if (role !== "user" && role !== "assistant") continue;
        // Flatten content blocks via contentText (drops thinking, truncates tool parts)
        const content = contentText(m.content ?? m.text ?? "");
        if (!content) continue;
        conversation.push({ role, content });
      }

      // Append assistant response.text if not already the last assistant msg
      const respText = entry.response?.text;
      if (respText && respText.trim().length >= 2) {
        const last = conversation[conversation.length - 1];
        if (!last || last.role !== "assistant" || last.content !== respText) {
          conversation.push({ role: "assistant", content: respText });
        }
      }

      if (conversation.length > 0) {
        turns.push({ turnId, conversation });
      }
    } catch {}
  }
  return turns;
}

async function main() {
  const input = await readStdin();
  const config = loadConfig();

  if (!config.connection.apiKey) {
    process.stdout.write("{}");
    return;
  }

  // ZCode Stop input uses camelCase: sessionId, turnId, cwd
  const sessionId = input?.sessionId || input?.session_id || "";
  const rawCwd = input?.cwd || process.env.CLAUDE_PROJECT_DIR || process.env.OMEM_PROJECT_DIR || "";
  const localRoot = rawCwd ? detectProjectRoot(rawCwd) : "";
  // Canonical (/mnt/...) form for everything sent to the server — keeps
  // WSL-side and Windows-side plugins on the same project identity.
  const cwd = localRoot ? toRemoteProjectPath(localRoot) : "";

  if (!sessionId) {
    logDebug("Stop: no sessionId, skipping", { inputKeys: Object.keys(input || {}) });
    process.stdout.write("{}");
    return;
  }

  // Session-level auto-store switch (toggled via MCP memory_toggle tool)
  if (!getAutoStore(sessionId)) {
    logDebug("Stop: auto-store OFF for session", { sessionId });
    process.stdout.write("{}");
    return;
  }

  // Keepalive ping for the detached web-server daemon. The daemon self-shuts
  // down after OMEM_WEB_IDLE_TIMEOUT_MS of inactivity; each Stop turn proves
  // zcode is still alive. If the daemon already died (long idle between turns),
  // re-spawn it so the web UI stays reachable mid-conversation.
  const webPort = config.web?.port || 5212;
  fetch(`http://127.0.0.1:${webPort}/health`)
    .then((r) => r.json())
    .then((body) => {
      if (body?.service !== "cerebro") throw new Error("wrong service");
    })
    .catch(() => {
      try {
        const webServerPath = join(dirname(fileURLToPath(import.meta.url)), "..", "web-server.js");
        const child = spawn(process.execPath, [webServerPath], {
          detached: true,
          stdio: "ignore",
          env: { ...process.env, OMEM_LOCAL_PORT: String(webPort) },
        });
        child.unref();
        logInfo("web-server daemon re-spawned", { port: webPort, pid: child.pid });
      } catch (err) {
        logError("web-server re-spawn failed", { error: String(err) });
      }
    });

  const client = new CerebroClient(config.connection.apiUrl, config.connection.apiKey, config);

  const allTurns = readRollout(sessionId);
  if (allTurns.length === 0) {
    logDebug("Stop: no rollout turns found", { sessionId });
    process.stdout.write("{}");
    return;
  }

  const state = loadState(sessionId);
  const newTurns = allTurns.filter((t) => !state.turnIds.has(t.turnId));

  if (newTurns.length === 0) {
    logDebug("Stop: no new turns to archive", { sessionId, totalTurns: allTurns.length });
    process.stdout.write("{}");
    return;
  }

  // Latest full snapshot from the most recent turn
  const latest = newTurns[newTurns.length - 1].conversation;

  // Incremental delta: slice past messages already ingested. If the snapshot
  // got SHORTER (context compaction reset), re-sync from the beginning — this
  // doubles as compact-summary ingestion (ZCode has no PreCompact/PostCompact).
  let delta;
  if (latest.length <= state.lastMsgCount) {
    delta = latest;
    logDebug("Stop: snapshot shrank (compact?), re-syncing full conversation", {
      sessionId,
      prev: state.lastMsgCount,
      now: latest.length,
    });
  } else {
    delta = latest.slice(state.lastMsgCount);
  }

  // Threshold gates on TOTAL conversation length (opencode session.idle
  // semantics) — short sessions wait, messages accumulate, nothing is lost.
  const threshold = config.ingest?.autoCaptureThreshold ?? 5;
  if (latest.length < threshold) {
    logDebug("Stop: below threshold, waiting for more messages", {
      sessionId,
      totalMsgs: latest.length,
      deltaMsgs: delta.length,
      threshold,
    });
    process.stdout.write("{}");
    return;
  }

  // Clean: strip inject-echo noise, drop fragments < 100 chars
  const messages = [];
  for (const m of delta) {
    const txt = cleanText(m.content);
    if (txt.length < 100) continue;
    messages.push({ role: m.role, content: txt });
  }

  let projectName;
  if (localRoot) projectName = await detectProjectName(localRoot);

  const agentId = process.env.OMEM_AGENT_ID || "zcode";

  logInfo("Stop: archiving session turns", {
    sessionId,
    newTurns: newTurns.length,
    deltaMsgs: delta.length,
    keptMsgs: messages.length,
    totalMsgs: latest.length,
    projectName,
    cwd: cwd || "(none)",
  });

  try {
    if (messages.length > 0) {
      await client.sessionIngest(messages, sessionId, agentId, undefined, projectName, cwd || undefined);
    }
    // Advance state only after success (or when everything was fragments)
    for (const t of newTurns) state.turnIds.add(t.turnId);
    saveState(sessionId, state.turnIds, latest.length);
    logInfo("Stop: session archived", {
      sessionId,
      turns: newTurns.length,
      msgs: messages.length,
      cursor: latest.length,
    });
  } catch (err) {
    logError("Stop: sessionIngest failed", { sessionId, error: String(err) });
  }

  process.stdout.write("{}");
}

main().catch((err) => {
  logError("Stop fatal", { error: String(err) });
  process.stdout.write("{}");
});
