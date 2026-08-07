#!/usr/bin/env node
// cerebro PostCompact hook — flush + ingest compact_summary + toast
// PostCompact 在 CC 完成上下文压缩后触发，stdin 包含 compact_summary。
import {
  config, flushSessionIngest, omPost, detectProjectName, detectProjectPath, parseStdinJSON, emit, logWarn, logError,
} from "./common.mjs";

if (!config.apiKey) { emit({}); process.exit(0); }

const input = parseStdinJSON();
const tp = input.transcript_path || "";
const sid = input.session_id || input.sessionId || "";
const summary = input.compact_summary || "";

if (!tp || !sid) {
  logWarn("post-compact: missing transcript_path/session_id");
  emit({});
  process.exit(0);
}

// 1. Flush 压缩前剩余增量（PreCompact 已 flush 过则 delta=0，无副作用）
const result = await flushSessionIngest(tp, sid).catch(() => ({ ok: false, count: 0 }));
if (!result.ok) logError(`post-compact: flush failed for sid=${sid}`);

// 2. Ingest compact_summary 作为 assistant 消息（对标 opencode autocontinueHook）
let summaryOk = false;
if (summary.length > 100) {
  const body = {
    messages: [{ role: "assistant", content: `[compact_summary] ${summary.slice(0, 8000)}` }],
    agent_id: process.env.OMEM_AGENT_ID || "claude-code",
    session_id: sid,
  };
  const pn = detectProjectName();
  const pp = detectProjectPath();
  if (pn) body.project_name = pn;
  if (pp) body.project_path = pp;
  const res = await omPost("/v1/memories/session-ingest", body, 25);
  summaryOk = res.status >= 200 && res.status < 300;
}

const totalCount = result.count + (summaryOk ? 1 : 0);
emit({
  systemMessage: `🧠 Cerebro · Post-compact · ${totalCount} items ingested`,
});
