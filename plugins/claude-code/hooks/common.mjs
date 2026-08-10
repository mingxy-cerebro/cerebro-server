// cerebro Claude Code plugin — shared base (Node cross-platform)
// Ported from common.sh — no bash/curl/python3 dependency. Pure Node.
// Config cascade: env > ~/.config/cerebro/config.json > builtin defaults
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, existsSync, mkdirSync, appendFileSync, unlinkSync, copyFileSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const PLUGIN_ROOT =
  process.env.CLAUDE_PLUGIN_ROOT ||
  resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Read version from package.json (single source of truth, no hardcoding)
let _pkgVersion = "unknown";
try {
  _pkgVersion = JSON.parse(readFileSync(join(PLUGIN_ROOT, "package.json"), "utf-8")).version || "unknown";
} catch {}
export const PLUGIN_VERSION = _pkgVersion;

const HOME = process.env.HOME || process.env.USERPROFILE || process.cwd();

// ─── Builtin defaults ────────────────────────────────────────────────────────
const DEF = {
  apiUrl: "https://www.mengxy.cc",
  requestTimeout: 15,
  recentCount: 8,
  searchCount: 8,
  maxContent: 3000,
  maxQueryLength: 200,
  logDir: join(HOME, ".config/cerebro/logs"),
  logEnabled: true,
  profileTimeoutMs: 2000,
  recentTimeoutMs: 3000,
};

// ─── Config cascade ──────────────────────────────────────────────────────────
function loadConfig() {
  let cfg = {};
  const cfgPath =
    process.env.CEREBRO_CONFIG_PATH || join(HOME, ".config/cerebro/config.json");
  try {
    if (existsSync(cfgPath)) {
      const raw = JSON.parse(readFileSync(cfgPath, "utf-8"));
      // Flat-config migration (legacy)
      if (raw.apiUrl && !raw.connection) {
        cfg = {
          connection: { apiUrl: raw.apiUrl, apiKey: raw.apiKey, requestTimeoutMs: raw.requestTimeoutMs },
          content: { maxQueryLength: raw.maxQueryLength, maxContentChars: raw.maxContentChars, maxContentLength: raw.maxContentLength },
          injection: { recentCount: raw.recentCount, searchCount: raw.searchCount },
          ingest: { autoCaptureThreshold: raw.autoCaptureThreshold, ingestMode: raw.ingestMode },
          logging: { logEnabled: raw.logEnabled, logLevel: raw.logLevel, logDir: raw.logDir },
        };
      } else {
        cfg = raw;
      }
    }
  } catch {}

  const c = cfg.connection || {};
  const i = cfg.injection || {};
  const ct = cfg.content || {};
  const lg = cfg.logging || {};

  const num = (env, cfgVal, def) => {
    const v = process.env[env];
    if (v && /^\d+$/.test(v)) return parseInt(v, 10);
    if (cfgVal) return typeof cfgVal === "number" ? cfgVal : parseInt(cfgVal, 10);
    return def;
  };

  return {
    apiUrl: (process.env.OMEM_API_URL || c.apiUrl || DEF.apiUrl).replace(/\/$/, ""),
    apiKey: process.env.OMEM_API_KEY || c.apiKey || "",
    requestTimeout: num("MEM_REQUEST_TIMEOUT", c.requestTimeoutMs ? c.requestTimeoutMs / 1000 : null, DEF.requestTimeout),
    recentCount: num("MEM_RECENT_COUNT", i.recentCount, DEF.recentCount),
    searchCount: num("MEM_SEARCH_COUNT", i.searchCount, DEF.searchCount),
    maxContent: num("MEM_MAX_CONTENT", ct.maxContentLength || ct.maxContentChars, DEF.maxContent),
    maxQueryLength: num("MEM_MAX_QUERY_LENGTH", ct.maxQueryLength, DEF.maxQueryLength),
    logDir: (process.env.MEM_LOG_DIR || lg.logDir || DEF.logDir).replace(/^~/, HOME),
    logEnabled: process.env.MEM_LOG_ENABLED
      ? process.env.MEM_LOG_ENABLED === "1"
      : lg.logEnabled !== undefined
        ? lg.logEnabled === true || lg.logEnabled === "1" || lg.logEnabled === 1
        : DEF.logEnabled,
    profileTimeoutMs: i.profileTimeoutMs || DEF.profileTimeoutMs,
    recentTimeoutMs: i.recentTimeoutMs || DEF.recentTimeoutMs,
  };
}

export const config = loadConfig();

// ─── Injection config (Claude Code specific, independent from opencode) ──────
// Three-level fallback: ~/.claude/cerebro.json > auto-init from bundled > bundled default
const CC_CONFIG_DIR = join(HOME, ".claude");
const CC_USER_CONFIG = join(CC_CONFIG_DIR, "cerebro.json");
const CC_BUNDLED_CONFIG = join(PLUGIN_ROOT, "config.json");

function loadInjectionConfig() {
  // 1. User config exists → read it
  if (existsSync(CC_USER_CONFIG)) {
    try {
      return JSON.parse(readFileSync(CC_USER_CONFIG, "utf-8"));
    } catch {}
  } else {
    // 2. First run → auto-initialize by copying bundled default
    try {
      mkdirSync(CC_CONFIG_DIR, { recursive: true });
      copyFileSync(CC_BUNDLED_CONFIG, CC_USER_CONFIG);
    } catch {}
  }
  // 3. Fallback → read bundled default
  try {
    return JSON.parse(readFileSync(CC_BUNDLED_CONFIG, "utf-8"));
  } catch {
    return {
      language: "en",
      recall: { enabled: true },
      nudge: { enabled: true },
      sessionStart: { profileEnabled: true, recentActivityEnabled: true },
    };
  }
}

export const injectionConfig = loadInjectionConfig();

// ─── HTTP (fetch-based, cross-platform) ──────────────────────────────────────
function headers(extra = {}) {
  return { "X-API-Key": config.apiKey, Accept: "application/json", ...extra };
}

export async function omGet(path, timeout = 8) {
  try {
    const resp = await fetch(`${config.apiUrl}${path}`, {
      headers: headers(),
      signal: AbortSignal.timeout(timeout * 1000),
    });
    return await resp.text();
  } catch {
    return '{"error":"request failed"}';
  }
}

export async function omPost(path, body, timeout) {
  timeout = timeout || config.requestTimeout;
  try {
    const resp = await fetch(`${config.apiUrl}${path}`, {
      method: "POST",
      headers: headers({ "Content-Type": "application/json" }),
      body: typeof body === "string" ? body : JSON.stringify(body),
      signal: AbortSignal.timeout(timeout * 1000),
    });
    return { ok: resp.ok, status: resp.status, text: await resp.text() };
  } catch {
    return { ok: false, status: 0, text: "" };
  }
}

export async function omHealth() {
  try {
    const resp = await fetch(`${config.apiUrl}/v1/stats`, {
      headers: headers(),
      signal: AbortSignal.timeout(5000),
    });
    return resp.ok;
  } catch {
    return false;
  }
}

// ─── Project / User Tagging ──────────────────────────────────────────────────
function sha256_16(input) {
  return createHash("sha256").update(input).digest("hex").slice(0, 16);
}

export function detectProjectPath() {
  try {
    const p = execSync("git rev-parse --show-toplevel", {
      encoding: "utf-8",
      timeout: 2000,
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
    if (p && p !== "/" && p !== HOME) return p;
  } catch {}
  return process.cwd();
}

export function containerTags() {
  let email = process.env.OMEM_USER_EMAIL;
  if (!email) {
    try {
      email = execSync("git config user.email", {
        encoding: "utf-8",
        timeout: 2000,
        stdio: ["pipe", "pipe", "pipe"],
      }).trim();
    } catch {}
  }
  const projectDir = detectProjectPath();
  const tags = [];
  if (email) tags.push(`omem_user_${sha256_16(email)}`);
  if (projectDir && projectDir !== HOME) tags.push(`omem_project_${sha256_16(projectDir)}`);
  return tags;
}

// ─── Logging ─────────────────────────────────────────────────────────────────
export function logWarn(msg) { _log("WARN", msg); }
export function logError(msg) { _log("ERROR", msg); }
export function logDebug(msg) { _log("DEBUG", msg); }

function _log(level, msg) {
  if (!config.logEnabled) return;
  try {
    const ts = new Date().toISOString();
    const logFile = join(config.logDir, "claude-code.log");
    mkdirSync(config.logDir, { recursive: true });
    appendFileSync(logFile, `${ts} ${level} ${msg}\n`);
  } catch {}
}

// ─── Web server refcount (multi-session lifecycle) ───────────────────────────
const REFCOUNT_FILE = join(HOME, ".config/cerebro/web-server.refcount");
const WEB_PID_FILE = join(HOME, ".config/cerebro/web-server.pid");

export function refCountInc() {
  try {
    const n = existsSync(REFCOUNT_FILE) ? parseInt(readFileSync(REFCOUNT_FILE, "utf-8").trim(), 10) || 0 : 0;
    writeFileSync(REFCOUNT_FILE, String(n + 1));
  } catch {}
}

export function refCountDec() {
  try {
    let n = existsSync(REFCOUNT_FILE) ? parseInt(readFileSync(REFCOUNT_FILE, "utf-8").trim(), 10) || 0 : 0;
    n = Math.max(0, n - 1);
    if (n === 0) {
      try {
        const pid = parseInt(readFileSync(WEB_PID_FILE, "utf-8").trim(), 10);
        if (pid) process.kill(pid, "SIGTERM");
      } catch {}
      try { unlinkSync(WEB_PID_FILE); } catch {}
      try { unlinkSync(REFCOUNT_FILE); } catch {}
    } else {
      writeFileSync(REFCOUNT_FILE, String(n));
    }
  } catch {}
}

// ─── Compact result handoff (PostCompact → SessionStart:compact) ─────────────
const COMPACT_RESULT_FILE = join(HOME, ".config/cerebro/last-compact-result.json");

export function writeCompactResult(result) {
  try {
    mkdirSync(dirname(COMPACT_RESULT_FILE), { recursive: true });
    writeFileSync(COMPACT_RESULT_FILE, JSON.stringify({ ...result, ts: Date.now() }));
  } catch {}
}

export function readCompactResult() {
  try {
    if (!existsSync(COMPACT_RESULT_FILE)) return null;
    const data = JSON.parse(readFileSync(COMPACT_RESULT_FILE, "utf-8"));
    // Stale after 60s
    if (Date.now() - (data.ts || 0) > 60_000) return null;
    return data;
  } catch {
    return null;
  }
}

// ─── Cursor (session ingest dedup) ───────────────────────────────────────────
const TRACKER_DIR = join(HOME, ".config/cerebro/trackers");

export function cursorGet(sessionId) {
  try {
    const f = join(TRACKER_DIR, `${sessionId}.txt`);
    if (existsSync(f)) return readFileSync(f, "utf-8").trim();
  } catch {}
  return "";
}

export function cursorSet(sessionId, lastId) {
  try {
    mkdirSync(TRACKER_DIR, { recursive: true });
    writeFileSync(join(TRACKER_DIR, `${sessionId}.txt`), lastId + "\n");
  } catch {}
}

// ─── Project name detection ──────────────────────────────────────────────────
export function detectProjectName() {
  const dir = detectProjectPath();
  const manifests = [
    ["package.json", "json"],
    ["composer.json", "json"],
    ["Cargo.toml", "toml"],
    ["pyproject.toml", "toml"],
    ["go.mod", "go"],
  ];
  for (const [mf, kind] of manifests) {
    const p = join(dir, mf);
    if (!existsSync(p)) continue;
    try {
      const txt = readFileSync(p, "utf-8");
      if (kind === "json") {
        const v = JSON.parse(txt).name;
        if (typeof v === "string" && v) return v.replace(/[^A-Za-z0-9_-]/g, "").slice(0, 32);
      } else if (kind === "toml") {
        const m = txt.match(/^name\s*=\s*"([^"]+)"/m);
        if (m) return m[1].replace(/[^A-Za-z0-9_-]/g, "").slice(0, 32);
      } else if (kind === "go") {
        const m = txt.match(/^module\s+(\S+)/m);
        if (m) {
          const name = m[1].replace(/\/+$/, "").split("/").pop();
          return name.replace(/[^A-Za-z0-9_-]/g, "").slice(0, 32);
        }
      }
    } catch {}
  }
  const basename = dir.replace(/\/+$/, "").split("/").pop() || "project";
  return basename.replace(/[^A-Za-z0-9_-]/g, "").slice(0, 32) || "project";
}

// ─── Hook helpers ────────────────────────────────────────────────────────────
export function readStdin() {
  try {
    return readFileSync(0, "utf-8");
  } catch {
    return "";
  }
}

export function parseStdinJSON() {
  try {
    const raw = readStdin();
    const parsed = raw ? JSON.parse(raw) : {};
    // CC hook 可能以 CLAUDE_PLUGIN_ROOT（缓存目录）为 cwd 运行，
    // 导致 detectProjectPath() 返回错误路径。
    // CC stdin 提供 cwd 字段（用户项目目录），chdir 到正确位置。
    if (parsed.cwd && parsed.cwd !== process.cwd()) {
      try { process.chdir(parsed.cwd); } catch {}
    }
    return parsed;
  } catch {
    return {};
  }
}

export function emit(obj) {
  process.stdout.write(JSON.stringify(obj));
}

// ─── Session ingest flush (shared by Stop + PreCompact) ──────────────────────
// Walks transcript JSONL past the saved cursor uuid, filters entries
// (strip inject-echo blocks, drop thinking, truncate tool_result/tool_use),
// POSTs the delta to /v1/memories/session-ingest, advances cursor on 2xx.
const INJECT_TAG_RE = /<(system-reminder|cerebro-[a-z0-9_-]+|supermemory-[a-z0-9_-]+)\b[^>]*>[\s\S]*?<\/\1>/gi;
const INJECT_SELF_RE = /<(system-reminder|cerebro-[a-z0-9_-]+|supermemory-[a-z0-9_-]+)\b[^>]*\/>/gi;
const WS_RE = /\s+/g;

export function cleanText(s) {
  if (typeof s !== "string") s = String(s);
  return s.replace(INJECT_TAG_RE, "").replace(INJECT_SELF_RE, "").replace(WS_RE, " ").trim();
}

function blockText(b) {
  const t = b.type;
  if (t === "text") return b.text || "";
  if (t === "thinking") return null;
  if (t === "tool_result") {
    let c = b.content || "";
    if (Array.isArray(c)) {
      c = c
        .map((x) => (typeof x === "object" && x?.type === "text" ? x.text || "" : typeof x === "string" ? x : ""))
        .join("\n");
    } else if (typeof c !== "string") {
      try { c = JSON.stringify(c); } catch { c = String(c); }
    }
    return "tool_result: " + (c || "").slice(0, 500);
  }
  if (t === "tool_use") {
    let inp;
    try { inp = JSON.stringify(b.input); } catch { inp = String(b.input); }
    return `tool_use(${b.name || "?"}): ${inp.slice(0, 100)}`;
  }
  return null;
}

function contentText(content) {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((b) => (typeof b === "object" && b !== null ? blockText(b) : null))
      .filter(Boolean)
      .join("\n");
  }
  return "";
}

export async function flushSessionIngest(transcriptPath, sessionId) {
  if (!transcriptPath || !existsSync(transcriptPath) || !sessionId || !config.apiKey) return { ok: false, count: 0 };

  const cursor = cursorGet(sessionId);
  const pn = detectProjectName();
  const pp = detectProjectPath();

  let entries = [];
  try {
    const lines = readFileSync(transcriptPath, "utf-8").split("\n");
    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      let d;
      try { d = JSON.parse(trimmed); } catch { continue; }
      if (d.type !== "user" && d.type !== "assistant") continue;
      const uid = d.uuid || "";
      const msg = d.message;
      if (typeof msg !== "object" || msg === null) continue;
      const role = msg.role;
      if (role !== "user" && role !== "assistant") continue;
      const raw = contentText(msg.content);
      entries.push({ uid, role, raw });
    }
  } catch {
    entries = [];
  }

  // Locate cursor
  let start = 0;
  if (cursor) {
    for (let idx = 0; idx < entries.length; idx++) {
      if (entries[idx].uid === cursor) {
        start = idx + 1;
        break;
      }
    }
  }

  const delta = entries.slice(start);
  if (delta.length === 0) return { ok: true, count: 0 }; // nothing new

  const lastUid = delta[delta.length - 1].uid;
  const messages = [];
  for (const { role, raw } of delta) {
    const txt = cleanText(raw);
    if (txt.length < 100) continue;
    messages.push({ role, content: txt });
  }

  const agentId = process.env.OMEM_AGENT_ID || "claude-code";

  if (messages.length > 0) {
    const body = { messages, agent_id: agentId };
    if (sessionId) body.session_id = sessionId;
    if (pn) body.project_name = pn;
    if (pp) body.project_path = pp;

    const result = await omPost("/v1/memories/session-ingest", body, 25);
    if (result.status >= 200 && result.status < 300) {
      cursorSet(sessionId, lastUid);
      logDebug(`flush_session_ingest: ok http=${result.status} cursor=${lastUid}`);
      return { ok: true, count: messages.length };
    } else {
      logError(`flush_session_ingest: http=${result.status} (cursor NOT advanced, will retry next run)`);
      return { ok: false, count: 0 };
    }
  } else {
    // All fragments — advance cursor anyway
    cursorSet(sessionId, lastUid);
    logDebug(`flush_session_ingest: 0 msgs kept (fragments), advancing cursor=${lastUid}`);
    return { ok: true, count: 0 };
  }
}

// ─── Sanitize / Truncate ─────────────────────────────────────────────────────
export function sanitizeContent(text, maxLen) {
  maxLen = maxLen || config.maxContent;
  let clean = text
    .replace(/<[\w-]+[^>]*>[\s\S]*?<\/[\w-]+>/g, "")
    .replace(/<[\w-]+[^>]*\/>/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return clean.length <= maxLen ? clean : clean.slice(0, maxLen) + "…[truncated]";
}

export function truncateQuery(text, len) {
  len = len || config.maxQueryLength;
  if (!text) return "";
  return text.length <= len ? text : text.slice(0, len);
}

// ─── Injection helpers (对标 opencode hooks.ts) ──────────────────────────────

const BOUNDARY_SEARCH_RATIO = 0.6;
const MAX_INJECTION_CHARS = 10000; // CC additionalContext 单字段上限

export function formatRelativeAge(isoDate) {
  if (!isoDate) return "unknown";
  const diffMs = Date.now() - new Date(isoDate).getTime();
  if (isNaN(diffMs)) return "unknown";
  const minutes = Math.floor(diffMs / 60000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  return `${Math.floor(days / 30)}mo ago`;
}

export function truncateAtBoundary(text, maxLength) {
  if (text.length <= maxLength) return text;
  const boundaries = /[.!?。！？\n]/;
  const searchEnd = Math.min(maxLength, text.length);
  for (let i = searchEnd - 1; i >= Math.floor(searchEnd * BOUNDARY_SEARCH_RATIO); i--) {
    if (boundaries.test(text[i])) return text.slice(0, i + 1).trimEnd() + "…";
  }
  let truncated = text.slice(0, maxLength);
  const lastCode = truncated.charCodeAt(truncated.length - 1);
  if (lastCode >= 0xd800 && lastCode <= 0xdbff) truncated = truncated.slice(0, -1);
  return truncated + "…";
}

// GET /v1/memories/search — 单路语义搜索
export async function searchMemories(query, limit, projectPath) {
  limit = limit || config.searchCount;
  const safeQ = truncateQuery(query);
  if (!safeQ) return [];
  const params = new URLSearchParams({ q: safeQ, limit: String(limit) });
  if (projectPath) params.set("project_path", projectPath);
  try {
    const resp = await fetch(`${config.apiUrl}/v1/memories/search?${params}`, {
      headers: { "X-API-Key": config.apiKey, Accept: "application/json" },
      signal: AbortSignal.timeout(5000),
    });
    const d = await resp.json();
    return d?.results || [];
  } catch {
    return [];
  }
}

// buildMemoryInjection — 对标 opencode hooks.ts:246-329
// 三路并发：profile + recent + search(query)。query 为空跳过 search。
export async function buildMemoryInjection(query, projectPath, options = {}) {
  const profileEnabled = options.profileEnabled !== false;
  const recentEnabled = options.recentEnabled !== false;
  const hdrs = { "X-API-Key": config.apiKey, Accept: "application/json" };
  const recentCount = config.recentCount;
  const searchCount = config.searchCount;
  const profileQs = projectPath ? `?project_path=${encodeURIComponent(projectPath)}` : "";
  const recentQs = `?limit=${recentCount}&offset=0&sort=updated_at&order=desc${projectPath ? `&project_path=${encodeURIComponent(projectPath)}` : ""}`;
  const safeQ = truncateQuery(query);

  const [profileResp, recentResp, searchResp] = await Promise.all([
    profileEnabled
      ? fetch(`${config.apiUrl}/v2/profile/inject${profileQs}`, { headers: hdrs, signal: AbortSignal.timeout(config.profileTimeoutMs) })
        .then((r) => r.text()).catch(() => "")
      : Promise.resolve(""),
    recentEnabled
      ? fetch(`${config.apiUrl}/v1/memories${recentQs}`, { headers: hdrs, signal: AbortSignal.timeout(config.recentTimeoutMs) })
        .then((r) => r.text()).catch(() => "")
      : Promise.resolve(""),
    safeQ
      ? fetch(`${config.apiUrl}/v1/memories/search?q=${encodeURIComponent(safeQ)}&limit=${searchCount}${projectPath ? `&project_path=${encodeURIComponent(projectPath)}` : ""}`, { headers: hdrs, signal: AbortSignal.timeout(5000) })
        .then((r) => r.text()).catch(() => "")
      : Promise.resolve(""),
  ]);

  // parse profile
  let profileContent = "";
  try {
    const pd = JSON.parse(profileResp);
    if (pd && !pd.error) profileContent = (pd.content || "").trim();
  } catch {}

  // parse recent (用完整 content，不用 l0_abstract)
  let projectMemories = [];
  try {
    const rd = JSON.parse(recentResp);
    if (rd && !rd.error) projectMemories = rd.memories || [];
  } catch {}

  // parse search
  let searchResults = [];
  try {
    const sd = JSON.parse(searchResp);
    if (sd && !sd.error) searchResults = sd.results || [];
  } catch {}

  // build [CEREBRO-MEMORY] block
  const sections = ["[CEREBRO-MEMORY]", ""];

  if (profileContent) {
    sections.push(profileContent);
    sections.push("");
  }

  const seenIds = new Set();
  if (projectMemories.length > 0) {
    sections.push("## Recent Project Activity");
    for (const m of projectMemories) {
      if (m.id) seenIds.add(m.id);
      const age = formatRelativeAge(m.updated_at || m.created_at);
      sections.push(`- (${age}) ${m.content || ""}`);
    }
    sections.push("");
  }

  const dedupedResults = searchResults.filter((r) => r.memory && !seenIds.has(r.memory.id));
  if (dedupedResults.length > 0) {
    sections.push("## Relevant Memories");
    for (const r of dedupedResults) {
      const age = formatRelativeAge(r.memory.created_at);
      sections.push(`- (${age}) ${r.memory.content || ""}`);
    }
    sections.push("");
  }

  sections.push("[/CEREBRO-MEMORY]");

  let text = sections.join("\n");
  // maxChars 截断保护
  if (text.length > MAX_INJECTION_CHARS) {
    const cutoff = text.lastIndexOf("\n", MAX_INJECTION_CHARS);
    text = text.slice(0, cutoff > 0 ? cutoff : MAX_INJECTION_CHARS) + "\n…\n[/CEREBRO-MEMORY]";
  }

  return {
    text,
    profileCount: profileContent ? 1 : 0,
    projectMemoryCount: projectMemories.length,
    searchCount: dedupedResults.length,
  };
}

// ─── POST recall-event (shared by session-start + user-prompt-submit) ─────────
// 让 web Sessions 页面看到每次注入的内容。对标 opencode chatMessageRecallHook createRecallEvent。
export async function postRecallEvent({ sessionId, recallType, queryText, profileInjected, keptCount, injectedContent, maxScore = 0 }) {
  if (!sessionId || !config.apiKey) return;
  try {
    await fetch(`${config.apiUrl}/v1/recall-events`, {
      method: "POST",
      headers: { "X-API-Key": config.apiKey, "Content-Type": "application/json" },
      body: JSON.stringify({
        session_id: sessionId,
        recall_type: recallType,
        query_text: (queryText || "").slice(0, 500),
        max_score: maxScore,
        llm_confidence: 0,
        profile_injected: profileInjected || false,
        kept_count: keptCount || 0,
        discarded_count: 0,
        injected_count: keptCount || 0,
        injected_content: (injectedContent || "").slice(0, 10000),
      }),
      signal: AbortSignal.timeout(5000),
    });
  } catch {}
}
