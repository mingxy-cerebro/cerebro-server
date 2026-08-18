#!/usr/bin/env node
// OMEM dream trigger — detached worker, spawned by flush-detached.mjs / session-start.mjs
// Runs independently after Claude Code terminates. Owns the full dream lifecycle:
//   lock → judge "enough material" → collect food → POST /v1/dreams → poll → write output
//
// State file protocol (~/.cache/cerebro/dream/state.json, tmp+rename atomic write):
//   { phase: "run"|"fail"|"done", job_id, started_at, updated_at,
//     last_dream_at, consumed, error?, stats? }
// Output: ~/.cache/cerebro/dream/output/<job_id>.json (full DreamResult)
// Lock:   ~/.cache/cerebro/dream/trigger.lock (O_EXCL, TTL 15min > 600s server budget)
//
// Memory food = CC local memory dir (one md per memory, frontmatter verbatim).
// Sessions food = mechanical extraction from transcripts (user/assistant text only,
// tool_use/tool_result/system stripped), per-session cap, oldest dropped first when
// over the 512KB / 50-session server budget (P1-2).
import { existsSync, mkdirSync, readFileSync, writeFileSync, openSync, closeSync, unlinkSync, readdirSync, statSync, renameSync } from "node:fs";
import { join, basename } from "node:path";
import { homedir } from "node:os";
import { config, logDebug, logError } from "./common.mjs";

const HOME = homedir();
const DREAM_DIR = process.env.OMEM_DREAM_DIR || join(HOME, ".cache", "cerebro", "dream");
const OUT_DIR = join(DREAM_DIR, "output");
const STATE = join(DREAM_DIR, "state.json");
const LOCK = join(DREAM_DIR, "trigger.lock");
const CONF = join(DREAM_DIR, "config.json");
// ponytail: hardcodes the home-project memory dir as the dream subject; DREAM_MEMORY_DIR
// escapes hatch for other projects if per-project dreams ever matter
const MEMORY_DIR = process.env.OMEM_DREAM_MEMORY_DIR || join(HOME, ".claude", "projects", "-home-dongx", "memory");
const PROJECTS_DIR = join(HOME, ".claude", "projects");

// Rhythm: dream when last dream ≥6h ago AND ≥2 new sessions since; 24h fallback.
const MIN_INTERVAL_MS = 6 * 3600 * 1000;
const FALLBACK_MS = 24 * 3600 * 1000;
const MIN_NEW_SESSIONS = 2;
const MAX_SESSIONS = 50;              // server hard limit (validate_request)
const MAX_PAYLOAD = 512 * 1024;       // server hard limit (P1-2)
const PER_SESSION_CAP = 16 * 1024;    // per-transcript extraction budget
const LOCK_TTL_MS = 15 * 60 * 1000;   // > 600s server job budget
const POLL_INTERVAL_MS = 2000;
const POLL_BUDGET_MS = 660 * 1000;    // > 600s server timeout (ADR-3)

// ─── runtime config: on/off switch + badge TTL ───────────────────────────────
// Absent file = defaults (on): fresh install dreams without any setup.
const DEFAULT_CONF = { enabled: true, badge_ttl_secs: 3600 };
export function readDreamConfig() {
  try { return { ...DEFAULT_CONF, ...JSON.parse(readFileSync(CONF, "utf8")) }; }
  catch { return { ...DEFAULT_CONF }; }
}

// ─── state helpers (tmp+rename atomic) ───────────────────────────────────────
export function readState() {
  try { return JSON.parse(readFileSync(STATE, "utf8")); } catch { return null; }
}
function writeState(obj) {
  mkdirSync(DREAM_DIR, { recursive: true });
  const tmp = STATE + ".tmp";
  writeFileSync(tmp, JSON.stringify(obj));
  renameSync(tmp, STATE);
}

// ─── trigger lock (O_EXCL; cross-session on shared fs; TOCTOU-proof) ─────────
function acquireLock() {
  mkdirSync(DREAM_DIR, { recursive: true });
  try { closeSync(openSync(LOCK, "wx")); return true; } // we hold it
  catch {
    try {
      const age = Date.now() - statSync(LOCK).mtimeMs;
      if (age > LOCK_TTL_MS) { unlinkSync(LOCK); try { closeSync(openSync(LOCK, "wx")); return true; } catch {} }
    } catch {}
    return false;
  }
}
function releaseLock() { try { unlinkSync(LOCK); } catch {} }

// ─── food: memory dir (verbatim md with frontmatter) ─────────────────────────
function collectMemory() {
  const parts = [];
  for (const f of readdirSync(MEMORY_DIR).filter((x) => x.endsWith(".md")).sort()) {
    try { parts.push(readFileSync(join(MEMORY_DIR, f), "utf8")); } catch {}
  }
  return parts.join("\n\n");
}

// ─── food: sessions (mechanical extraction, no LLM in hooks) ─────────────────
function listTranscripts() {
  const out = [];
  for (const proj of readdirSync(PROJECTS_DIR)) {
    const dir = join(PROJECTS_DIR, proj);
    let st; try { st = statSync(dir); } catch { continue; }
    if (!st.isDirectory()) continue;
    for (const f of readdirSync(dir)) {
      if (!f.endsWith(".jsonl")) continue;
      const p = join(dir, f);
      let s; try { s = statSync(p); } catch { continue; }
      out.push({ path: p, mtime: s.mtimeMs });
    }
  }
  return out.sort((a, b) => b.mtime - a.mtime); // newest first
}

function extractTranscript(path) {
  const lines = readFileSync(path, "utf8").split("\n");
  const texts = [];
  let budget = PER_SESSION_CAP;
  for (const line of lines) {
    if (budget <= 0) break;
    let j; try { j = JSON.parse(line); } catch { continue; }
    if (j.type !== "user" && j.type !== "assistant") continue;
    const c = j.message?.content;
    let t = "";
    if (typeof c === "string") t = c;
    else if (Array.isArray(c)) for (const blk of c) { if (blk.type === "text") t += blk.text + "\n"; }
    t = t.trim();
    if (!t || t.startsWith("<")) continue; // skip hook injections / tool wrappers
    t = t.slice(0, budget);
    budget -= t.length;
    texts.push(t);
  }
  return texts.join("\n---\n");
}

// ─── rhythm judge ────────────────────────────────────────────────────────────
export function judgeMaterial(state) {
  const last = state?.last_dream_at ? Date.parse(state.last_dream_at) : 0;
  const newer = listTranscripts().filter((t) => t.mtime > last);
  const age = Date.now() - last;
  const enoughTime = age >= MIN_INTERVAL_MS;
  const fallback = last > 0 && age >= FALLBACK_MS;
  const remaining_ms = last ? Math.max(0, MIN_INTERVAL_MS - age) : null;
  return { ok: newer.length >= MIN_NEW_SESSIONS && (enoughTime || !last) || fallback, since: last ? new Date(last).toISOString() : null, count: newer.length, remaining_ms };
}

// ─── statusline badge: all state→label/color decisions live here, bash renders ─
// off grey / run orange (zombie run >30min falls through) / fail red until TTL /
// rdy green (gates met) / accumulating `[n/2]·3h 41m` (needs n sessions + time).
const RUN_ZOMBIE_MS = 30 * 60 * 1000; // > lock TTL 15min + poll 11min: live run can't outlive it
function fmtRemain(ms) {
  if (ms == null || ms <= 0) return "";
  const m = Math.ceil(ms / 60000);
  return m < 1 ? "<1m" : m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
}
export function badgeLine() {
  const conf = readDreamConfig();
  if (!conf.enabled) return { text: "cerebro dream off", color: 90 };
  const st = readState();
  const age = st?.updated_at ? Date.now() - Date.parse(st.updated_at) : Infinity;
  if (st?.phase === "run" && age < RUN_ZOMBIE_MS) return { text: "cerebro dream run", color: 208 };
  if (st?.phase === "fail" && age < conf.badge_ttl_secs * 1000) return { text: "cerebro dream fail", color: 196 };
  const judge = judgeMaterial(st);
  if (judge.ok) return { text: "cerebro dream rdy", color: 71 };
  const rem = fmtRemain(judge.remaining_ms);
  return { text: `cerebro dream [${Math.min(judge.count, MIN_NEW_SESSIONS)}/${MIN_NEW_SESSIONS}]${rem ? "·" + rem : ""}`, color: 250 };
}

// ─── detached main: trigger + poll + persist ─────────────────────────────────
export async function runDream() {
  if (!config.apiKey) return;
  if (!readDreamConfig().enabled) { logDebug("dream: disabled by config"); return; }
  if (!acquireLock()) { logDebug("dream: lock held, another window is dreaming"); return; }
  try {
    const prev = readState();
    const judge = judgeMaterial(prev);
    if (!judge.ok) { logDebug(`dream: not enough material (new=${judge.count})`); return; }

    const memory = collectMemory();
    const trans = listTranscripts().filter((t) => !judge.since || t.mtime > Date.parse(judge.since));
    const sessions = [];
    let bytes = memory.length;
    for (const t of trans) { // newest first; drop oldest by simply stopping
      if (sessions.length >= MAX_SESSIONS) break;
      const s = extractTranscript(t.path);
      if (bytes + s.length > MAX_PAYLOAD) break;
      bytes += s.length;
      sessions.push(s);
    }
    if (!sessions.length) { logDebug("dream: no session food extracted"); return; }

    const started = new Date().toISOString();
    writeState({ phase: "run", job_id: null, started_at: started, updated_at: started, last_dream_at: prev?.last_dream_at || null, consumed: false });

    let resp;
    try {
      resp = await fetch(`${config.apiUrl}/v1/dreams`, {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-API-Key": config.apiKey },
        body: JSON.stringify({ memory, sessions, since: judge.since }),
        signal: AbortSignal.timeout(30_000),
      });
    } catch (e) {
      writeState({ phase: "fail", error: `POST: ${e?.message || e}`, updated_at: new Date().toISOString(), last_dream_at: prev?.last_dream_at || null, consumed: true });
      logError(`dream: POST failed ${e?.message || e}`);
      return;
    }
    if (!resp.ok) {
      const body = await resp.text().catch(() => "");
      writeState({ phase: "fail", error: `POST http=${resp.status} ${body.slice(0, 200)}`, updated_at: new Date().toISOString(), last_dream_at: prev?.last_dream_at || null, consumed: true });
      logError(`dream: POST http=${resp.status}`);
      return;
    }
    const { id } = await resp.json();
    const now = new Date().toISOString();
    const st = readState() || {};
    writeState({ ...st, phase: "run", job_id: id, updated_at: now }); // job_id immediately: orphans are queryable
    logDebug(`dream: job ${id} accepted`);

    // poll to terminal state (2s × 660s > 600s server budget)
    const deadline = Date.now() + POLL_BUDGET_MS;
    while (Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS));
      let jr;
      try {
        const g = await fetch(`${config.apiUrl}/v1/dreams/${id}`, { headers: { "X-API-Key": config.apiKey }, signal: AbortSignal.timeout(10_000) });
        if (g.status === 404) continue; // server restart lost the job — keep polling til budget, then fail
        jr = await g.json();
      } catch { continue; }
      if (jr.status === "completed" || jr.status === "failed") {
        const ts = new Date().toISOString();
        if (jr.status === "completed" && jr.result) {
          mkdirSync(OUT_DIR, { recursive: true });
          const outPath = join(OUT_DIR, `${id}.json`);
          writeFileSync(outPath, JSON.stringify(jr.result));
          writeState({ phase: "done", job_id: id, started_at: st.started_at, updated_at: ts, last_dream_at: ts, consumed: false, stats: jr.result.stats, output: outPath });
          logDebug(`dream: completed stats=${JSON.stringify(jr.result.stats)}`);
        } else {
          writeState({ phase: "fail", job_id: id, error: jr.error || "job failed", updated_at: ts, last_dream_at: st.started_at, consumed: true });
          logError(`dream: job failed ${jr.error || ""}`);
        }
        return;
      }
    }
    writeState({ phase: "fail", job_id: id, error: "poll budget exhausted", updated_at: new Date().toISOString(), last_dream_at: st.started_at, consumed: true });
    logError("dream: poll budget exhausted");
  } finally {
    releaseLock();
  }
}

// ─── report-side helpers (used by session-start.mjs) ─────────────────────────
// Mark consumed so each new window doesn't re-inject the same report.
export function writeStateForReport(st) {
  writeState({ ...st, consumed: true });
}

// Orphan recovery: single GET, no polling. completed → write output+done, return new state.
// running/pending → give up (next window retries). Network errors → return null (stale shown grey).
export async function fetchOrphanResult(st) {
  if (!st?.job_id || !config.apiKey) return null;
  try {
    const g = await fetch(`${config.apiUrl}/v1/dreams/${st.job_id}`, {
      headers: { "X-API-Key": config.apiKey },
      signal: AbortSignal.timeout(6000), // single GET, 6s budget — same as recent-timeout precedent
    });
    const jr = await g.json();
    if (jr.status !== "completed" || !jr.result) return null; // running/failed → next window
    mkdirSync(OUT_DIR, { recursive: true });
    const outPath = join(OUT_DIR, `${st.job_id}.json`);
    writeFileSync(outPath, JSON.stringify(jr.result));
    const ts = new Date().toISOString();
    const ns = { ...st, phase: "done", updated_at: ts, last_dream_at: ts, consumed: false, stats: jr.result.stats, output: outPath };
    writeState(ns);
    return ns;
  } catch { return null; }
}

// ─── direct CLI entry (detached worker) ──────────────────────────────────────
if (process.argv[1] && basename(process.argv[1]) === "dream.mjs") {
  if (process.argv[2] === "--badge") { console.log(JSON.stringify(badgeLine())); process.exit(0); }
  if (!existsSync(LOCK) && judgeMaterial(readState()).ok) {
    await runDream();
  }
  process.exit(0);
}
