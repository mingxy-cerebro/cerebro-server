import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { homedir, tmpdir } from "node:os";
import { openSync, closeSync, unlinkSync, statSync, writeSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { loadPluginConfig } from "./config.js";

const execFileAsync = promisify(execFile);
const require = createRequire(import.meta.url);

// ── Version fetching ────────────────────────────────────────────────

async function getLatestVersion(): Promise<string | null> {
  try {
    const { stdout } = await execFileAsync("npm", ["view", "@mingxy/cerebro", "version"], { timeout: 10000 });
    return stdout.trim();
  } catch { return null; }
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
  try {
    // 1. Download tgz
    await execFileAsync("npm", ["pack", "@mingxy/cerebro@latest", "--pack-destination", tmpDir], { timeout: 60000 });

    // 2. Find the tgz file
    const files = readdirSync(tmpDir);
    const tgz = files.find(f => f.endsWith(".tgz"));
    if (!tgz) return false;

    // 3. Extract to target dir (strip the "package/" prefix from tgz)
    await execFileAsync("tar", ["-xzf", join(tmpDir, tgz), "-C", targetDir, "--strip-components=1", "--no-same-owner", "--no-same-permissions"], { timeout: 30000 });

    return true;
  } catch {
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
      }
    } catch { /* lock file doesn't exist, normal */ }

    lockFd = openSync(LOCK_FILE, "wx");
    writeSync(lockFd, String(process.pid));
    return true;
  } catch { return false; }
}

function releaseLock(): void {
  if (lockFd !== null) {
    try { closeSync(lockFd); } catch {}
    lockFd = null;
    try { unlinkSync(LOCK_FILE); } catch {}
  }
}

// ── Main entry ──────────────────────────────────────────────────────

export async function checkAndUpdate(tui: any, currentVersion: string): Promise<void> {
  const config = loadPluginConfig();
  if (!config.autoUpdate) return;

  const latest = await getLatestVersion();
  if (!latest) return;

  if (compareVersions(latest, currentVersion) <= 0) return; // already up to date

  if (!acquireLock()) return; // another process is updating

  try {
    const targetDir = getInstallDir();
    const success = await installUpdate(targetDir);

    if (success) {
      try {
        tui?.showToast?.({
          body: { message: `Cerebro updated to v${latest} — restart opencode to apply`, variant: "info" }
        });
      } catch {}
    }
  } finally {
    releaseLock();
  }
}
