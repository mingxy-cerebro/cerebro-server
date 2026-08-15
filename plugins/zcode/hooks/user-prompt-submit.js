#!/usr/bin/env node
// UserPromptSubmit hook — reasoned recall instruction + keyword nudge.
// Port of plugins/claude-code/hooks/user-prompt-submit.mjs, adapted for ZCode.
//
// IMPORTANT: per-message keyword-driven memory search + auto-inject is the
// DEPRECATED opencode autoRecall pattern and is intentionally NOT implemented
// here. This hook makes ZERO search API calls — it only injects static
// instruction text (config-driven ~/.zcode/cerebro.json) that teaches the
// model when to call memory_search itself, plus save/recall keyword nudges.
//
// Input (stdin): { session_id/sessionId, prompt, cwd }
// Output (stdout):
//   { "hookSpecificOutput": { "hookEventName": "UserPromptSubmit", "additionalContext": "..." } }

import { loadConfig, loadInjectionConfig } from "./lib/config.js";
import { CerebroClient } from "./lib/cerebro-client.js";
import { logDebug, logError } from "./lib/logger.js";

function readStdin() {
  return new Promise((resolve) => {
    let data = "";
    if (process.stdin.isTTY) return resolve({});
    process.stdin.setEncoding("utf-8");
    process.stdin.on("data", (chunk) => (data += chunk));
    process.stdin.on("end", () => {
      try {
        resolve(data.trim() ? JSON.parse(data) : {});
      } catch {
        resolve({});
      }
    });
    process.stdin.on("error", () => resolve({}));
    setTimeout(() => resolve({}), 2000);
  });
}

async function main() {
  const input = await readStdin();
  const prompt = typeof input?.prompt === "string" ? input.prompt : input?.message || "";
  const sid = input?.sessionId || input?.session_id || "";
  const config = loadConfig();
  const injectionCfg = loadInjectionConfig();

  const parts = [];

  // Recall instruction (static text — no API call)
  const recallCfg = injectionCfg.recall || {};
  if (recallCfg.enabled !== false && recallCfg.prompt) {
    parts.push(`<cerebro-recall>\n${recallCfg.prompt}\n</cerebro-recall>`);
  }

  // Keyword nudge (static text — no API call)
  const nudgeCfg = injectionCfg.nudge || {};
  if (nudgeCfg.enabled !== false) {
    const pLow = prompt.toLowerCase();
    const nudges = [];
    const saveKw = nudgeCfg.saveKeywords || [];
    const recallKw = nudgeCfg.recallKeywords || [];

    if (saveKw.some((kw) => pLow.includes(String(kw).toLowerCase())) && nudgeCfg.savePrompt) {
      nudges.push(`<cerebro-nudge>${nudgeCfg.savePrompt}</cerebro-nudge>`);
    }
    if (recallKw.some((kw) => pLow.includes(String(kw).toLowerCase())) && nudgeCfg.recallPrompt) {
      nudges.push(`<cerebro-nudge>${nudgeCfg.recallPrompt}</cerebro-nudge>`);
    }
    if (nudges.length) parts.push(nudges.join("\n"));
  }

  const injectionText = parts.join("\n\n");

  // Fire-and-forget recall-event so the web Sessions page shows hook activity
  // (port of claude-code postRecallEvent; never blocks, never fails loudly)
  if (sid && config.connection.apiKey) {
    try {
      const client = new CerebroClient(config.connection.apiUrl, config.connection.apiKey, config);
      client
        .createRecallEvent({
          session_id: sid,
          recall_type: "auto",
          query_text: prompt.slice(0, 500),
          max_score: 0,
          llm_confidence: 0,
          profile_injected: false,
          kept_count: 0,
          discarded_count: 0,
          injected_count: 0,
          injected_content: injectionText.slice(0, 10000),
        })
        .catch(() => {});
    } catch (err) {
      logError("postRecallEvent setup failed", { error: String(err) });
    }
  }

  logDebug("UserPromptSubmit", { injected: injectionText.length > 0, textLen: injectionText.length });

  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "UserPromptSubmit",
        additionalContext: injectionText,
      },
    }),
  );
}

main().catch((err) => {
  logError("UserPromptSubmit fatal", { error: String(err) });
  process.stdout.write("{}");
});
