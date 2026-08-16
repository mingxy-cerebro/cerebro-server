#!/usr/bin/env node
// cerebro SessionStart hook — profile + recent injection + recall event + web server
import { spawn } from "node:child_process";
import { join } from "node:path";
import { readFileSync } from "node:fs";
import {
  config, PLUGIN_ROOT, PLUGIN_VERSION, detectProjectPath, parseStdinJSON, emit, buildMemoryInjection, postRecallEvent, refCountInc, readCompactResult, injectionConfig,
} from "./common.mjs";
import { judgeMaterial, readState, writeStateForReport, fetchOrphanResult } from "./dream.mjs";

// ─── dream report: read local output, mark consumed; never write memory here ──
// URGENT is ratio-based: dropped >10% of total OR added ≥2 → flag for the
// session's Claude to surface to the user. The full new archive stays on disk —
// writing memory files is the session Claude's job, after the user reviews.
async function dreamReport() {
  try {
    let st = readState();
    if (!st) return "";
    // orphan: run state older than 15min → one single GET, no polling
    if (st.phase === "run" && Date.now() - Date.parse(st.updated_at) > 15 * 60 * 1000 && st.job_id) {
      st = (await fetchOrphanResult(st)) || st;
    }
    if (st.phase === "fail") {
      return `[dream-report] last dream failed: ${st.error || "?"} (${st.updated_at})\n`;
    }
    if (st.phase !== "done" || st.consumed || !st.output) return "";
    const result = JSON.parse(readFileSync(st.output, "utf8"));
    const s = result.stats || {};
    const total = s.total || 1;
    const urgent = (s.dropped || 0) > total * 0.1 || (s.added || 0) >= 2;
    const lines = [
      `[dream-report] job ${st.job_id?.slice(0, 8)} · ${st.updated_at}`,
      `merged ${s.merged || 0} / updated ${s.updated || 0} / added ${s.added || 0} / dropped ${s.dropped || 0} / kept ${total - (s.merged || 0) - (s.updated || 0) - (s.added || 0) - (s.dropped || 0)}`,
    ];
    for (const e of result.entries || []) {
      lines.push(`· ${e.action || "?"} ${e.name || "?"} — ${e.description || ""}`);
    }
    if (urgent) lines.push(`⚠ URGENT: significant dream changes above — surface this report to the user NOW.`);
    lines.push(`New full archive on disk: ${st.output}`);
    lines.push(`Do NOT auto-overwrite memory files: show the user a diff, let them approve, then write.`);
    writeStateForReport(st); // consumed=true
    return lines.join("\n") + "\n";
  } catch { return ""; }
}

// ─── dream follow-up: material ready but no window ended recently? start one ──
// Async spawn, never blocks startup. Complements the SessionEnd trigger.
function dreamFollowUp() {
  try {
    const st = readState();
    if (st?.phase === "run" || (st?.phase === "done" && !st.consumed)) return; // busy or unreported
    if (!judgeMaterial(st).ok) return;
    const child = spawn(process.execPath, [join(PLUGIN_ROOT, "hooks", "dream.mjs")], {
      detached: true, stdio: "ignore", env: { ...process.env },
    });
    child.unref();
  } catch {}
}

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
const ss = injectionConfig.sessionStart || {};
const injection = await buildMemoryInjection("", pp, {
  profileEnabled: ss.profileEnabled !== false,
  recentEnabled: ss.recentActivityEnabled !== false,
});

// CEREBRO-TIME 注入 Claude 上下文（Claude 需要时间感知）
let out = injection.text;
const timeLine = cerebroTime();
out = out.replace("[CEREBRO-MEMORY]", `[CEREBRO-MEMORY]\n${timeLine}`);

// CEREBRO-STATUS 通过 systemMessage 显示给用户（Q2: toast 替代方案）
const memCount = injection.projectMemoryCount + injection.searchCount;
let statusMsg = injection.recentFailed
  ? `🧠 Cerebro v${PLUGIN_VERSION} · Recent ✗ (timeout) · Profile ${injection.profileCount > 0 ? "✓" : "✗"}`
  : `🧠 Cerebro v${PLUGIN_VERSION} · Connected · ${memCount} memories · Profile ${injection.profileCount > 0 ? "✓" : "✗"}`;

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
  failureReason: injection.recentFailed ? "recent fetch failed/timeout" : "",
});

// ─── dream：报告注入（本地读，零网络）+ 料够补刀（异步 spawn）───────────────────
const drep = await dreamReport();
if (drep) {
  out = drep + out;
  statusMsg += " · Dream report ready";
}
dreamFollowUp();

emit({
  systemMessage: statusMsg,
  hookSpecificOutput: { hookEventName: "SessionStart", additionalContext: out },
});
