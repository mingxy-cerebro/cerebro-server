#!/usr/bin/env node
// cerebro detached flush — spawned by session-end.mjs to survive process exit
// Runs independently after Claude Code terminates. No stdin/stdout to Claude.
import { existsSync } from "node:fs";
import { config, flushSessionIngest, clearPendingClearFlush, logDebug, logError } from "./common.mjs";
import { judgeMaterial, readState, runDream } from "./dream.mjs";

const tp = process.env.CEREBRO_TP || "";
const sid = process.env.CEREBRO_SID || "";

if (!tp || !sid || !config.apiKey) {
  logError(`detached flush: skipped (tp=${!!tp} sid=${!!sid} key=${!!config.apiKey})`);
  process.exit(0);
}

if (!existsSync(tp)) {
  logError(`detached flush: transcript not found sid=${sid} tp=${tp}`);
  process.exit(0);
}

const result = await flushSessionIngest(tp, sid).catch((err) => {
  logError(`detached flush: exception sid=${sid} err=${err?.message || err}`);
  return { ok: false, count: 0 };
});

if (result.ok) {
  logDebug(`detached flush: ok count=${result.count} sid=${sid}`);
  // Retrying a SessionStart(clear) handoff — consume it so the next SessionStart
  // doesn't replay a 0-delta toast from the stale pending file.
  if (process.env.CEREBRO_CLEAR_PENDING) clearPendingClearFlush();
} else {
  logError(`detached flush: failed sid=${sid} http=${result.status || "?"} (cursor NOT advanced, will retry next session)`);
}

// ─── dream trigger: session just ended = new material; judge then run ────────
// This detached worker already survives CC exit, so it also survives the
// ≤660s dream poll — no second spawn needed.
try {
  const judge = judgeMaterial(readState());
  if (judge.ok) {
    logDebug(`dream: material ready (new=${judge.count}), dreaming after flush`);
    await runDream();
  }
} catch (e) {
  logError(`dream trigger: ${e?.message || e}`);
}

process.exit(0);
