#!/usr/bin/env node
// Stop hook — auto-archive session at conversation turn end.
// ZCode fires Stop after each assistant response. Unlike Claude Code, ZCode does
// NOT pass transcript_path via stdin. Instead it provides sessionId + turnId.
// We read the conversation from ZCode's rollout JSONL:
//   ~/.zcode/cli/rollout/model-io-sess_<sessionId>.jsonl
// Each line = one turn (request.messages = full snapshot, response.text = reply).
// We dedup by turnId so each turn is ingested exactly once.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { loadConfig } from "./lib/config.js";
import { CerebroClient } from "./lib/cerebro-client.js";
import { logInfo, logError, logDebug, logWarn } from "./lib/logger.js";
import { getUserTag, getProjectTag, detectProjectName } from "./lib/util.js";

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

function processedPath(sessionId) {
  return join(stateDir(), `processed-turns-${sessionId || "default"}.json`);
}

function loadProcessedTurns(sessionId) {
  try {
    const data = JSON.parse(readFileSync(processedPath(sessionId), "utf-8"));
    return new Set(Array.isArray(data.turnIds) ? data.turnIds : []);
  } catch {
    return new Set();
  }
}

function saveProcessedTurns(sessionId, turnIds) {
  try {
    mkdirSync(stateDir(), { recursive: true });
    writeFileSync(processedPath(sessionId), JSON.stringify({ turnIds: [...turnIds], ts: Date.now() }));
  } catch {}
}

// Remove <system-reminder> blocks and injected noise that would pollute memory.
function stripSystemNoise(text) {
  if (!text) return "";
  let clean = text.replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, "");
  clean = clean.replace(/<EXTREMELY_IMPORTANT>[\s\S]*?<\/EXTREMELY_IMPORTANT>/g, "");
  clean = clean.replace(/^SessionStart hook additional context:[\s\S]*$/m, "");
  return clean.trim();
}

// Read rollout JSONL → [{turnId, conversation:[{role,content}]}]
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
        let content = m.content;
        if (Array.isArray(content)) {
          content = content
            .filter((p) => p && (p.type === "text" || typeof p === "string"))
            .map((p) => (typeof p === "string" ? p : p.text || ""))
            .join("\n");
        }
        if (typeof content !== "string") continue;
        const cleaned = stripSystemNoise(content);
        if (!cleaned || cleaned.trim().length < 2) continue;
        conversation.push({ role, content: cleaned.slice(0, 4000) });
      }

      // Append assistant response.text if not already the last assistant msg
      const respText = entry.response?.text;
      if (respText && respText.trim().length >= 2) {
        const last = conversation[conversation.length - 1];
        if (!last || last.role !== "assistant" || last.content !== respText) {
          conversation.push({ role: "assistant", content: respText.slice(0, 4000) });
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
  const cwd = input?.cwd || process.env.CLAUDE_PROJECT_DIR || process.env.OMEM_PROJECT_DIR || "";

  if (!sessionId) {
    logDebug("Stop: no sessionId, skipping", { inputKeys: Object.keys(input || {}) });
    process.stdout.write("{}");
    return;
  }

  // Keepalive ping for the detached web-server daemon. The daemon self-shuts
  // down after OMEM_WEB_IDLE_TIMEOUT_MS of inactivity; each Stop turn proves
  // zcode is still alive and refreshes the daemon's idle timer.
  const webPort = config.web?.port || 5212;
  fetch(`http://127.0.0.1:${webPort}/health`).catch(() => {});

  const client = new CerebroClient(config.connection.apiUrl, config.connection.apiKey, config);

  const allTurns = readRollout(sessionId);
  if (allTurns.length === 0) {
    logDebug("Stop: no rollout turns found", { sessionId });
    process.stdout.write("{}");
    return;
  }

  const processed = loadProcessedTurns(sessionId);
  const newTurns = allTurns.filter((t) => !processed.has(t.turnId));

  if (newTurns.length === 0) {
    logDebug("Stop: no new turns to archive", { sessionId, totalTurns: allTurns.length });
    process.stdout.write("{}");
    return;
  }

  const allMessages = [];
  for (const t of newTurns) {
    for (const m of t.conversation) allMessages.push(m);
  }

  const threshold = config.ingest?.autoCaptureThreshold ?? 5;
  if (allMessages.length < threshold) {
    logDebug("Stop: below threshold, persisting state", {
      sessionId,
      msgCount: allMessages.length,
      threshold,
    });
    for (const t of newTurns) processed.add(t.turnId);
    saveProcessedTurns(sessionId, processed);
    process.stdout.write("{}");
    return;
  }

  let projectName;
  if (cwd) projectName = await detectProjectName(cwd);

  const agentId = process.env.OMEM_AGENT_ID || "zcode";

  logInfo("Stop: archiving session turns", {
    sessionId,
    newTurns: newTurns.length,
    msgCount: allMessages.length,
    projectName,
    cwd: cwd || "(none)",
  });

  try {
    await client.sessionIngest(allMessages, sessionId, agentId, undefined, projectName, cwd || undefined);
    for (const t of newTurns) processed.add(t.turnId);
    saveProcessedTurns(sessionId, processed);
    logInfo("Stop: session archived", { sessionId, turns: newTurns.length, msgs: allMessages.length });
  } catch (err) {
    logError("Stop: sessionIngest failed", { sessionId, error: String(err) });
  }

  process.stdout.write("{}");
}

main().catch((err) => {
  logError("Stop fatal", { error: String(err) });
  process.stdout.write("{}");
});
