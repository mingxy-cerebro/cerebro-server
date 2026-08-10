#!/usr/bin/env node
// cerebro detached flush — spawned by session-end.mjs to survive process exit
// Runs independently after Claude Code terminates. No stdin/stdout to Claude.
import { existsSync } from "node:fs";
import { config, flushSessionIngest, logDebug, logError } from "./common.mjs";

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
} else {
  logError(`detached flush: failed sid=${sid} http=${result.status || "?"} (cursor NOT advanced, will retry next session)`);
}

process.exit(0);
