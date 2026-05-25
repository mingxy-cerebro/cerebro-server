import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { homedir, tmpdir } from "node:os";
import { openSync, closeSync, unlinkSync, statSync, writeSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { loadPluginConfig } from "./config.js";
import { logInfo, logDebug, logError } from "./logger.js";

const execFileAsync = promisify(execFile);
const require = createRequire(import.meta.url);

// ── Version fetching ────────────────────────────────────────────────

async function getLatestVersion(): Promise<string | null> {
  try {
    const { stdout } = await execFileAsync("npm", ["view", "@mingxy/cerebro", "version"], { timeout: 10000 });
    logDebug("updater: fetched latest version", { version: stdout.trim() });
    return stdout.trim();
  } catch {
    logError("updater: failed to fetch latest version");
    return null;
  }
}

// ── Semantic version comparison ─────────────────────────────────────

function compareVersions(a: string, b: string): number {
  const pa = a.replace(/^v/, "").split(".").map(Number);
  const pb = b.replace(/^v/, "").split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] ?? 0) > (pb[i] ?? 0)) return 1;
    if ((pa[i] ?? 0) < (pb[i] ?? 0)) return -1;
  }
  return 0;
}

// ── Install dir detection ───────────────────────────────────────────

function getInstallDir(): string {
  try {
    const pkgPath = require.resolve("@mingxy/cerebro/package.json");
    return dirname(pkgPath);
  } catch {
    return join(homedir(), ".cache", "opencode", "packages", "@mingxy", "cerebro");
  }
}

// ── Install update via npm pack + tar ───────────────────────────────

async function installUpdate(targetDir: string): Promise<boolean> {
  const tmpDir = mkdtempSync(join(tmpdir(), "cerebro-update-"));
  logInfo("updater: downloading update", { targetDir, tmpDir });
  try {
    // 1. Download tgz
    await execFileAsync("npm", ["pack", "@mingxy/cerebro@latest", "--pack-destination", tmpDir], { timeout: 60000 });

    // 2. Find the tgz file
    const files = readdirSync(tmpDir);
    const tgz = files.find(f => f.endsWith(".tgz"));
    if (!tgz) {
      logError("updater: no tgz found after npm pack", { tmpDir, files: files.join(",") });
      return false;
    }

    // 3. Extract to target dir (strip the "package/" prefix from tgz)
    await execFileAsync("tar", ["-xzf", join(tmpDir, tgz), "-C", targetDir, "--strip-components=1", "--no-same-owner", "--no-same-permissions"], { timeout: 30000 });

    logInfo("updater: update installed successfully", { targetDir, tgz });
    return true;
  } catch (err) {
    logError("updater: install failed", { error: err instanceof Error ? err.message : String(err) });
    return false;
  } finally {
    try { rmSync(tmpDir, { recursive: true }); } catch {}
  }
}

// ── File lock with stale lock cleanup ───────────────────────────────

const LOCK_FILE = join(tmpdir(), "cerebro-update.lock");
const STALE_LOCK_MS = 5 * 60 * 1000; // 5 minutes
let lockFd: number | null = null;

function acquireLock(): boolean {
  try {
    // Clean up stale lock first
    try {
      const s = statSync(LOCK_FILE);
      if (Date.now() - s.mtimeMs > STALE_LOCK_MS) {
        unlinkSync(LOCK_FILE);
        logDebug("updater: cleaned stale lock", { ageMs: Date.now() - s.mtimeMs });
      }
    } catch { /* lock file doesn't exist, normal */ }

    lockFd = openSync(LOCK_FILE, "wx");
    writeSync(lockFd, String(process.pid));
    logDebug("updater: lock acquired", { pid: process.pid });
    return true;
  } catch {
    logDebug("updater: lock acquisition failed (another process updating?)");
    return false;
  }
}

function releaseLock(): void {
  if (lockFd !== null) {
    try { closeSync(lockFd); } catch {}
    lockFd = null;
    try { unlinkSync(LOCK_FILE); } catch {}
    logDebug("updater: lock released");
  }
}

// ── Main entry ──────────────────────────────────────────────────────

export async function checkAndUpdate(tui: any, currentVersion: string): Promise<void> {
  const config = loadPluginConfig();
  if (!config.autoUpdate) {
    logDebug("updater: autoUpdate disabled, skipping");
    return;
  }

  logInfo("updater: checking for updates", { currentVersion });

  const latest = await getLatestVersion();
  if (!latest) return;

  if (compareVersions(latest, currentVersion) <= 0) {
    logInfo("updater: already up to date", { currentVersion, latest });
    return;
  }

  logInfo("updater: new version available", { currentVersion, latest });

  if (!acquireLock()) return;

  try {
    const targetDir = getInstallDir();
    const success = await installUpdate(targetDir);

    if (success) {
      logInfo("updater: update completed", { from: currentVersion, to: latest });
      try {
        tui?.showToast?.({
          body: { message: `Cerebro updated to v${latest} — restart opencode to apply`, variant: "info" }
        });
      } catch {}
    } else {
      logError("updater: update failed", { targetDir });
    }
  } finally {
    releaseLock();
  }
}
