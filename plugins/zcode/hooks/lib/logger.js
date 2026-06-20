// File logger — ported from plugins/opencode/src/logger.ts
// Writes to <logDir>/cerebro-zcode.log, 5MB rolling, 7-day expiry.
import { appendFileSync, mkdirSync, statSync, existsSync, readdirSync, unlinkSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const LEVEL_MAP = { DEBUG: 0, INFO: 1, WARN: 2, ERROR: 3 };
const MAX_FILE_SIZE = 5 * 1024 * 1024;

function readConfiguredLevel() {
  try {
    const raw = JSON.parse(readFileSync(join(homedir(), ".config", "cerebro", "config.json"), "utf-8"));
    const level = raw?.logging?.logLevel ?? raw?.logLevel ?? "INFO";
    return LEVEL_MAP[level] ?? LEVEL_MAP.INFO;
  } catch {
    return LEVEL_MAP.INFO;
  }
}

const MIN_LEVEL = readConfiguredLevel();
const LOG_DIR = join(homedir(), ".config", "cerebro", "logs");
const LOG_PATH = join(LOG_DIR, "cerebro-zcode.log");

function writeLog(level, message, fields) {
  const lvl = LEVEL_MAP[level] ?? 0;
  if (lvl < MIN_LEVEL) return;
  try {
    mkdirSync(LOG_DIR, { recursive: true });
    try {
      if (existsSync(LOG_PATH) && statSync(LOG_PATH).size > MAX_FILE_SIZE) {
        unlinkSync(LOG_PATH);
      }
    } catch {}
    const ts = new Date().toISOString().replace("T", " ").replace(/\.\d+Z$/, "");
    const parts = [`${level.padEnd(5)} ${ts} service=cerebro-zcode ${message}`];
    if (fields) {
      for (const [k, v] of Object.entries(fields)) {
        parts.push(`${k}=${typeof v === "string" ? v : JSON.stringify(v)}`);
      }
    }
    appendFileSync(LOG_PATH, parts.join(" ") + "\n");
  } catch {
    // best-effort logging; never throw
  }
}

export function logDebug(msg, fields) { writeLog("DEBUG", msg, fields); }
export function logInfo(msg, fields) { writeLog("INFO", msg, fields); }
export function logWarn(msg, fields) { writeLog("WARN", msg, fields); }
export function logError(msg, fields) { writeLog("ERROR", msg, fields); }

// One-time cleanup of logs older than 7 days (fire-and-forget)
setTimeout(() => {
  try {
    const cutoff = Date.now() - 7 * 86400000;
    for (const f of readdirSync(LOG_DIR)) {
      if (!f.startsWith("cerebro")) continue;
      const fp = join(LOG_DIR, f);
      try {
        if (statSync(fp).mtimeMs < cutoff) unlinkSync(fp);
      } catch {}
    }
  } catch {}
}, 5000).unref?.();
