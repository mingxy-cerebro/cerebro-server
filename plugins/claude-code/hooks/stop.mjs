#!/usr/bin/env node
// cerebro Stop hook — periodic session-ingest flush every N turns
// Does NOT fire on Ctrl+C interrupt (Claude Code design).
// SessionEnd detached process covers the interrupt case.
import { parseStdinJSON, emit, flushSessionIngest, stopCounterGet, stopCounterSet, injectionConfig } from "./common.mjs";

const input = parseStdinJSON();
const tp = input.transcript_path || "";
const sid = input.session_id || "";

const stopCfg = injectionConfig.stopFlush || {};
if (stopCfg.enabled === false || !tp || !sid) {
  emit({});
  process.exit(0);
}

const interval = stopCfg.interval || 5;
const counter = stopCounterGet(sid);
const newCount = counter + 1;

if (newCount < interval) {
  stopCounterSet(sid, newCount);
  emit({});
  process.exit(0);
}

// Threshold reached — flush
stopCounterSet(sid, 0);
const result = await flushSessionIngest(tp, sid).catch(() => ({ ok: false, count: 0 }));

emit({
  systemMessage: result.ok
    ? `🧠 Cerebro · Auto-saved · ${result.count} messages ingested`
    : `🧠 Cerebro · Auto-save failed (will retry next flush)`,
});
