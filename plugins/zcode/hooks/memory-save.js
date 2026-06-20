#!/usr/bin/env node
// Manual session archive — invoked by /memory-save command.
// Reads the full session conversation from rollout JSONL and ingests it
// in one shot (ignores the N-round threshold — this is a user-initiated save).

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { loadConfig } from "./lib/config.js";
import { CerebroClient } from "./lib/cerebro-client.js";
import { logInfo, logError } from "./lib/logger.js";
import { getUserTag, getProjectTag, detectProjectName } from "./lib/util.js";

function stripSystemNoise(text) {
  if (!text) return "";
  let clean = text.replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, "");
  clean = clean.replace(/<EXTREMELY_IMPORTANT>[\s\S]*?<\/EXTREMELY_IMPORTANT>/g, "");
  return clean.trim();
}

// Read the LAST rollout snapshot to get the most complete conversation.
function readSessionMessages(sessionId) {
  if (!sessionId) return [];
  const sid = sessionId.replace(/^sess_/, "");
  const rolloutPath = join(homedir(), ".zcode", "cli", "rollout", `model-io-sess_${sid}.jsonl`);
  let raw;
  try {
    raw = readFileSync(rolloutPath, "utf-8");
  } catch {
    return [];
  }

  const lines = raw.split(/\r?\n/).filter(Boolean);
  // Take the last line (most complete snapshot)
  for (let i = lines.length - 1; i >= 0; i--) {
    try {
      const entry = JSON.parse(lines[i]);
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
      return conversation;
    } catch {}
  }
  return [];
}

async function main() {
  const config = loadConfig();
  if (!config.connection.apiKey) {
    console.error("[cerebro] OMEM_API_KEY not set — cannot save memory.");
    process.exit(1);
  }

  // sessionId comes from env (set by command) or first arg
  const sessionId = process.env.ZCODE_SESSION_ID || process.argv[2] || "";
  const cwd = process.env.ZCODE_CWD || process.env.CLAUDE_PROJECT_DIR || process.cwd();

  if (!sessionId) {
    console.error("[cerebro] No sessionId. Usage: memory-save.js <sessionId>");
    process.exit(1);
  }

  const client = new CerebroClient(config.connection.apiUrl, config.connection.apiKey, config);
  const messages = readSessionMessages(sessionId);

  if (messages.length === 0) {
    console.error(`[cerebro] No conversation found for session ${sessionId}.`);
    process.exit(1);
  }

  const projectName = cwd ? await detectProjectName(cwd) : undefined;
  const agentId = process.env.OMEM_AGENT_ID || "zcode";

  console.error(`[cerebro] Saving ${messages.length} messages from session ${sessionId}...`);

  try {
    await client.sessionIngest(messages, sessionId, agentId, undefined, projectName, cwd || undefined);
    console.error(`[cerebro] ✓ Session archived (${messages.length} messages).`);
  } catch (err) {
    console.error(`[cerebro] ✗ Archive failed: ${err instanceof Error ? err.message : String(err)}`);
    process.exit(1);
  }
}

main().catch((err) => {
  console.error(`[cerebro] Fatal: ${err instanceof Error ? err.message : String(err)}`);
  process.exit(1);
});
