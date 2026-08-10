#!/usr/bin/env node
// cerebro UserPromptSubmit hook — reasoned recall instruction + keyword nudge
// Injection content is config-driven (~/.claude/cerebro.json), not hardcoded.
import { parseStdinJSON, emit, postRecallEvent, injectionConfig } from "./common.mjs";

const input = parseStdinJSON();
const prompt = typeof input === "object" ? input.prompt || input.message || "" : "";
const sid = input.session_id || "";

// ─── Build injection from config ────────────────────────────────────────────
const parts = [];

// Recall instruction
const recallCfg = injectionConfig.recall || {};
if (recallCfg.enabled !== false && recallCfg.prompt) {
  parts.push(`<cerebro-recall>\n${recallCfg.prompt}\n</cerebro-recall>`);
}

// Keyword nudge
const nudgeCfg = injectionConfig.nudge || {};
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

// ─── POST recall-event (web Sessions page) ───────────────────────────────────
await postRecallEvent({
  sessionId: sid,
  recallType: "auto",
  queryText: prompt,
  profileInjected: false,
  keptCount: 0,
  injectedContent: injectionText,
  maxScore: 0,
});

emit({ hookSpecificOutput: { hookEventName: "UserPromptSubmit", additionalContext: injectionText } });
