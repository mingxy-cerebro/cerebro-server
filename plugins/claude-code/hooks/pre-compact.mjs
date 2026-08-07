#!/usr/bin/env node
// cerebro PreCompact hook — flush delta before compaction
// PreCompact 不支持 hookSpecificOutput.additionalContext（CC schema 限制）。
// 仅做 flushSessionIngest，压缩指导由 CLAUDE.md 中的 cerebro-recall 指令覆盖。
import { config, flushSessionIngest, parseStdinJSON, emit, logWarn, logError } from "./common.mjs";

if (!config.apiKey) { emit({}); process.exit(0); }

const input = parseStdinJSON();
const tp = input.transcript_path || "";
const sid = input.session_id || input.sessionId || "";

if (!tp || !sid) {
  logWarn("pre-compact: missing transcript_path/session_id");
  emit({});
  process.exit(0);
}

const ok = await flushSessionIngest(tp, sid).catch(() => false);
if (!ok) logError(`pre-compact: flush_session_ingest failed for sid=${sid} (will retry at SessionEnd)`);

emit({});
