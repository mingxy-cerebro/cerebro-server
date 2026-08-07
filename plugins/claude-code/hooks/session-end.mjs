#!/usr/bin/env node
// cerebro SessionEnd hook — final flush of session transcript
// Q3=A: 替代每轮 Stop hook 的 ingest。会话结束时做最终 flush，
// 确保未触发 PreCompact 的短会话也能保存增量记忆。
// SessionEnd 无法阻止会话终止，仅做最大努力 flush。
import { config, flushSessionIngest, refCountDec, parseStdinJSON, emit, logWarn, logError } from "./common.mjs";

if (!config.apiKey) { emit({}); process.exit(0); }

const input = parseStdinJSON();
const tp = input.transcript_path || "";
const sid = input.session_id || input.sessionId || "";

if (!tp || !sid) {
  logWarn("session-end: missing transcript_path/session_id");
  emit({});
  process.exit(0);
}

const result = await flushSessionIngest(tp, sid).catch(() => ({ ok: false, count: 0 }));
if (!result.ok) logError(`session-end: flush_session_ingest failed for sid=${sid}`);

refCountDec();
emit({
  systemMessage: result.ok
    ? `🧠 Cerebro · Session saved · ${result.count} messages ingested`
    : `🧠 Cerebro · Session ingest failed (will retry next time)`,
});
