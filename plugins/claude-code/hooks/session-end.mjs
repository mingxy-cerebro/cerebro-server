#!/usr/bin/env node
// cerebro SessionEnd hook — spawn detached flush to survive process exit
// Claude Code does NOT wait for SessionEnd hooks to complete ("cannot block
// session termination"). Direct await fetch gets killed → http=0.
// Fix: spawn detached child process, parent exits immediately.
import { spawn } from "node:child_process";
import { join } from "node:path";
import { config, PLUGIN_ROOT, removeLive, parseStdinJSON, emit, logDebug, writePendingClearFlush } from "./common.mjs";

if (!config.apiKey) { emit({}); process.exit(0); }

const input = parseStdinJSON();
const tp = input.transcript_path || "";
const sid = input.session_id || input.sessionId || "";
const reason = input.reason || "";

logDebug(`session-end: sid=${sid} reason=${reason} tp=${tp ? tp.slice(-40) : "EMPTY"}`);

// /clear swaps session_id and fires SessionStart(clear) right after — hand the
// flush to it so the result lands in that hook's toast (detached flush is mute).
if (reason === "clear" && tp && sid) {
  writePendingClearFlush(tp, sid);
  removeLive(sid);
  emit({});
  process.exit(0);
}

// Spawn detached flush script — survives parent exit
if (tp && sid) {
  const flushScript = join(PLUGIN_ROOT, "hooks", "flush-detached.mjs");
  try {
    const child = spawn(process.execPath, [flushScript], {
      detached: true,
      stdio: "ignore",
      env: { ...process.env, CEREBRO_TP: tp, CEREBRO_SID: sid },
    });
    child.unref();
  } catch {}
}

removeLive(sid);
emit({
  systemMessage: `🧠 Cerebro · Session flush in background`,
});
