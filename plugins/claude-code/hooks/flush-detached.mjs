#!/usr/bin/env node
// cerebro detached flush — spawned by session-end.mjs to survive process exit
// Runs independently after Claude Code terminates. No stdin/stdout to Claude.
import { config, flushSessionIngest, logDebug, logError } from "./common.mjs";

const tp = process.env.CEREBRO_TP || "";
const sid = process.env.CEREBRO_SID || "";

if (!tp || !sid || !config.apiKey) process.exit(0);

const result = await flushSessionIngest(tp, sid).catch(() => ({ ok: false, count: 0 }));

if (result.ok) {
  logDebug(`detached flush: ok count=${result.count} sid=${sid}`);
} else {
  logError(`detached flush: failed sid=${sid} (cursor NOT advanced, will retry next session)`);
}

process.exit(0);
