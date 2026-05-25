import {
  appendFileSync,
  mkdirSync,
  existsSync,
  statSync,
  renameSync,
  readdirSync,
  unlinkSync,
} from "node:fs";
import { join } from "node:path";
import { loadPluginConfig } from "./config.js";

// ── Level map ─────────────────────────────────────────────────────────

const LEVEL_MAP: Record<string, number> = {
  DEBUG: 0,
  INFO: 1,
  WARN: 2,
  ERROR: 3,
};

// ── Config access with 30-second TTL cache ────────────────────────────

let cachedConfig: ReturnType<typeof loadPluginConfig> | null = null;
let configCacheTime = 0;
const CONFIG_TTL_MS = 30_000;

function getConfig() {
  const now = Date.now();
  if (cachedConfig && (now - configCacheTime) < CONFIG_TTL_MS) return cachedConfig;
  cachedConfig = loadPluginConfig();
  configCacheTime = now;
  return cachedConfig;
}

function getLogFilePath(sessionId?: string): string {
  const base = getConfig().logging.logDir;
  if (sessionId) {
    return join(base, `cerebro-${sessionId}.log`);
  }
  return join(base, "cerebro.log");
}

function getMinLevel(): number {
  return LEVEL_MAP[getConfig().logging.logLevel] ?? LEVEL_MAP.INFO;
}

function isLogEnabled(): boolean {
  return getConfig().logging.logEnabled;
}

// ── Opencode client for dual-track logging ────────────────────────────

let opencodeClient: any = null;

export function setOpencodeClient(client: any): void {
  opencodeClient = client;
}

// ── Log file rotation (5 MB threshold) ────────────────────────────────
// NOTE: multi-window concurrent rotate is a known limitation — the last
// writer to rename wins; earlier writers will create a fresh file.

const MAX_LOG_SIZE = 5 * 1024 * 1024; // 5 MB

function rotateIfNeeded(logFile: string): void {
  try {
    const s = statSync(logFile);
    if (s.size > MAX_LOG_SIZE) {
      renameSync(logFile, logFile.replace(".log", ".old.log"));
    }
  } catch { /* file doesn't exist yet, first write */ }
}

// ── Startup cleanup (delete logs older than 7 days) ───────────────────

const LOG_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000; // 7 days

function cleanupOldLogs(): void {
  const logDir = getConfig().logging.logDir;
  try {
    const files = readdirSync(logDir);
    const cutoff = Date.now() - LOG_MAX_AGE_MS;
    for (const f of files) {
      if (!f.endsWith(".log") && !f.endsWith(".old.log")) continue;
      const fp = join(logDir, f);
      try {
        const s = statSync(fp);
        if (s.mtimeMs < cutoff) unlinkSync(fp);
      } catch {}
    }
  } catch {}
}

// Run cleanup once at module load
cleanupOldLogs();

// ── Core logging ──────────────────────────────────────────────────────

let lastLogTime = Date.now();

function ensureLogDir(logDir: string): void {
  if (!existsSync(logDir)) {
    try {
      mkdirSync(logDir, { recursive: true });
    } catch {}
  }
}

function writeLog(level: string, message: string, fields?: Record<string, unknown>): void {
  if (!isLogEnabled()) return;
  const lvl = LEVEL_MAP[level] ?? 0;
  if (lvl < getMinLevel()) return;

  const cfg = getConfig();
  const sid = (fields?.sessionId ?? fields?.sessionID) as string | undefined;
  const logFile = getLogFilePath(sid);
  ensureLogDir(cfg.logging.logDir);

  const now = new Date();
  const nowMs = now.getTime();
  const delta = ((nowMs - lastLogTime) / 1000).toFixed(2);
  lastLogTime = nowMs;
  const pad = (n: number) => String(n).padStart(2, "0");
  const ts = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
  const parts = [`${level.padEnd(5)} ${ts} +${delta}s service=cerebro`];
  if (fields) {
    for (const [k, v] of Object.entries(fields)) {
      const val = typeof v === "string" ? v : JSON.stringify(v);
      parts.push(`${k}=${val}`);
    }
  }
  parts.push(message);

  // Track 1: file
  try {
    rotateIfNeeded(logFile);
    appendFileSync(logFile, parts.join(" ") + "\n");
  } catch {}

  // Track 2: opencode client
  try {
    opencodeClient?.app?.log({
      body: { service: "cerebro", level: level.toLowerCase(), message, extra: fields },
    });
  } catch { /* opencode client not available, skip */ }
}

// ── Public API ────────────────────────────────────────────────────────

export function logInfo(message: string, fields?: Record<string, unknown>): void {
  writeLog("INFO", message, fields);
}

export function logWarn(message: string, fields?: Record<string, unknown>): void {
  writeLog("WARN", message, fields);
}

export function logError(message: string, fields?: Record<string, unknown>): void {
  writeLog("ERROR", message, fields);
}

export function logDebug(message: string, fields?: Record<string, unknown>): void {
  writeLog("DEBUG", message, fields);
}
