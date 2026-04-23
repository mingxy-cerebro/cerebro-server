import { appendFileSync, mkdirSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const LOG_DIR = join(homedir(), ".config", "ourmem");
const LOG_FILE = join(LOG_DIR, "plugin.log");

function ensureLogDir(): void {
  if (!existsSync(LOG_DIR)) {
    try {
      mkdirSync(LOG_DIR, { recursive: true });
    } catch {
      // silently fail if we can't create log directory
    }
  }
}

function writeLog(level: string, ...args: unknown[]): void {
  ensureLogDir();
  const timestamp = new Date().toISOString();
  const message = args
    .map((a) => (typeof a === "string" ? a : JSON.stringify(a)))
    .join(" ");
  const line = `[${timestamp}] [${level}] ${message}\n`;
  try {
    appendFileSync(LOG_FILE, line);
  } catch {
    // silently fail if we can't write to log file
  }
}

export function logInfo(...args: unknown[]): void {
  writeLog("INFO", ...args);
}

export function logWarn(...args: unknown[]): void {
  writeLog("WARN", ...args);
}

export function logError(...args: unknown[]): void {
  writeLog("ERROR", ...args);
}

export function logDebug(...args: unknown[]): void {
  writeLog("DEBUG", ...args);
}
