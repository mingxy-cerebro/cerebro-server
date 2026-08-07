#!/usr/bin/env node
// cerebro Stop hook — every-turn transcript delta ingest
import { config, flushSessionIngest, parseStdinJSON, emit, logWarn, logError } from "./common.mjs";

if (!config.apiKey) { emit({}); process.exit(0); }

const input = parseStdinJSON();
const tp = input.transcript_path || "";
const sid = input.session_id || input.sessionId || "";

if (!tp || !sid) {
  logWarn("stop: missing transcript_path/session_id in hook input");
  emit({});
  process.exit(0);
}

const ok = await flushSessionIngest(tp, sid).catch(() => false);
if (!ok) logError(`stop: flush_session_ingest failed for sid=${sid} (will retry next turn)`);

emit({});
