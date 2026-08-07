#!/usr/bin/env node
// cerebro SessionStart hook — profile + recent injection + recall event + web server
import { spawn } from "node:child_process";
import { join } from "node:path";
import {
  config, PLUGIN_ROOT, PLUGIN_VERSION, detectProjectPath, parseStdinJSON, emit, buildMemoryInjection, postRecallEvent, refCountInc, readCompactResult,
} from "./common.mjs";

const input = parseStdinJSON();
const sid = input.session_id || "";
const startSource = input.source || ""; // "startup" | "resume" | "clear" | "compact"

// ─── web server 拉起（probe + detached spawn，跨平台）─────────────────────────
const webPort = process.env.OMEM_LOCAL_PORT || "5212";
try {
  const resp = await fetch(`http://127.0.0.1:${webPort}/health`, {
    signal: AbortSignal.timeout(1000),
  });
  if (!resp.ok) throw new Error("not healthy");
} catch {
  try {
    const child = spawn(
      process.execPath,
      [join(PLUGIN_ROOT, "scripts", "web-server.mjs")],
      { detached: true, stdio: "ignore", env: { ...process.env } },
    );
    child.unref();
  } catch {}
}
refCountInc();

// ─── API key 检查 ────────────────────────────────────────────────────────────
if (!config.apiKey) {
  const msg = `[cerebro] OMEM_API_KEY not set — memory is disabled.

To enable persistent memory, set your API key:
  export OMEM_API_KEY="your-key"

Get a free key:
  curl -X POST ${config.apiUrl}/v1/tenants -H "Content-Type: application/json" -d "{}"

Then restart Claude Code.`;
  emit({ systemMessage: "🧠 Cerebro: API key not set — memory disabled", hookSpecificOutput: { hookEventName: "SessionStart", additionalContext: msg } });
  process.exit(0);
}

// ─── 时间标记 ────────────────────────────────────────────────────────────────
function cerebroTime() {
  const d = new Date(Date.now() + 8 * 3600 * 1000);
  const pad = (n) => String(n).padStart(2, "0");
  const days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  return `[CEREBRO-TIME] 当前: ${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())} ${days[d.getUTCDay()]}`;
}

// ─── buildMemoryInjection（对标 opencode buildMemoryInjection）──────────────────
const pp = detectProjectPath();
const injection = await buildMemoryInjection("", pp); // SessionStart 无 query → 只 profile + recent

// CEREBRO-TIME 注入 Claude 上下文（Claude 需要时间感知）
let out = injection.text;
const timeLine = cerebroTime();
out = out.replace("[CEREBRO-MEMORY]", `[CEREBRO-MEMORY]\n${timeLine}`);

// CEREBRO-STATUS 通过 systemMessage 显示给用户（Q2: toast 替代方案）
const memCount = injection.projectMemoryCount + injection.searchCount;
let statusMsg = `🧠 Cerebro v${PLUGIN_VERSION} · Connected · ${memCount} memories · Profile ${injection.profileCount > 0 ? "✓" : "✗"}`;

// After compact, PostCompact toast gets overridden by SessionStart:compact toast.
// Merge PostCompact ingest result into this toast so user sees it.
if (startSource === "compact") {
  const cr = readCompactResult();
  if (cr) {
    statusMsg += ` · Post-compact ingest ${cr.ok ? "✓" : "✗"} ${cr.count} items`;
  }
}

// ─── POST recall event（让 web sessions 页面看到 CC session + 完整注入内容）─────
await postRecallEvent({
  sessionId: sid,
  recallType: "session_start",
  queryText: `Session Start · ${injection.projectMemoryCount} memories · ${injection.profileCount > 0 ? "profile" : "no profile"}`,
  profileInjected: injection.profileCount > 0,
  keptCount: injection.projectMemoryCount,
  injectedContent: out,
});

emit({
  systemMessage: statusMsg,
  hookSpecificOutput: { hookEventName: "SessionStart", additionalContext: out },
});
