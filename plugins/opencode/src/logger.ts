import { appendFileSync, mkdirSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const LEVEL_MAP: Record<string, number> = {
  DEBUG: 0,
  INFO: 1,
  WARN: 2,
  ERROR: 3,
};

const MIN_LEVEL = LEVEL_MAP[process.env.OMEM_LOG_LEVEL?.toUpperCase() ?? ""] ?? LEVEL_MAP.INFO;

const LOG_DIR = join(homedir(), ".config", "ourmem");
const LOG_FILE = join(LOG_DIR, "plugin.log");

const START_TIME = Date.now();

function ensureLogDir(): void {
  if (!existsSync(LOG_DIR)) {
    try {
      mkdirSync(LOG_DIR, { recursive: true });
    } catch {}
  }
}

function writeLog(level: string, message: string, fields?: Record<string, unknown>): void {
  const lvl = LEVEL_MAP[level] ?? 0;
  if (lvl < MIN_LEVEL) return;
  ensureLogDir();
  const now = new Date();
  const ts = now.toISOString().replace("T", " ").replace(/\.\d+Z$/, "");
  const offset = Date.now() - START_TIME;
  const parts = [`${level.padEnd(5)} ${ts} +${offset}ms service=cerebro`];
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
