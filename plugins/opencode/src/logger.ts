import { appendFileSync, mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { loadPluginConfig } from "./config.js";

const LEVEL_MAP: Record<string, number> = {
  DEBUG: 0,
  INFO: 1,
  WARN: 2,
  ERROR: 3,
};

const cfg = loadPluginConfig();
const MIN_LEVEL = LEVEL_MAP[cfg.logging.logLevel] ?? LEVEL_MAP.INFO;
const LOG_DIR = cfg.logging.logDir;
const LOG_FILE = join(LOG_DIR, "plugin.log");
const LOG_ENABLED = cfg.logging.logEnabled;

let lastLogTime = Date.now();

function ensureLogDir(): void {
  if (!existsSync(LOG_DIR)) {
    try {
      mkdirSync(LOG_DIR, { recursive: true });
    } catch {}
  }
}

function writeLog(level: string, message: string, fields?: Record<string, unknown>): void {
  if (!LOG_ENABLED) return;
  const lvl = LEVEL_MAP[level] ?? 0;
  if (lvl < MIN_LEVEL) return;
  ensureLogDir();
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
  try {
    appendFileSync(LOG_FILE, parts.join(" ") + "\n");
  } catch {}
}

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
