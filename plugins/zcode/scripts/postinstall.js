#!/usr/bin/env node
// postinstall hook — runs automatically after `npm install @mingxy/cerebro-zcode`.
// Copies the plugin assets to ~/.zcode/plugins/cerebro/ and registers the path
// in ~/.zcode/cli/config.json (plugins.dirs). After this, restarting ZCode loads
// the plugin automatically (source:"inline", defaultEnabled:true).
//
// v0.3.0: zero npm dependencies (MCP included) — nothing to build or install
// after copying. MCP loads from the plugin's own .mcp.json; any legacy global
// mcp.servers registration is cleaned up to avoid double-spawning.
//
// Safety: this is a no-op when run from inside the plugin's own dev directory
// (detected via npm_lifecycle_event / cwd), so `npm install` during local
// development does not self-install.

import { cpSync, existsSync, readFileSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
// scripts/ is one level below package root
const PKG_ROOT = resolve(__dirname, "..");
const ZCODE_DIR = join(homedir(), ".zcode");
const CONFIG_PATH = join(ZCODE_DIR, "cli", "config.json");
const TARGET = join(homedir(), ".zcode", "plugins", "cerebro");

// Skip self-install in these cases (dev / pack / no-tty CI build)
const lifecycle = process.env.npm_lifecycle_event || "";
const isSelfDev =
  lifecycle === "postinstall" && existsSync(join(PKG_ROOT, "package.json")) &&
  // Heuristic: if the package root is NOT inside a node_modules, we're in dev
  !PKG_ROOT.replace(/\\/g, "/").includes("/node_modules/");

// Allow forcing via env (npm install --ignore-scripts bypasses this entirely)
const FORCE = process.env.CEREBRO_ZCODE_INSTALL === "1";

function readConfig() {
  try {
    return JSON.parse(readFileSync(CONFIG_PATH, "utf-8"));
  } catch {
    return {};
  }
}

function writeConfig(cfg) {
  mkdirSync(dirname(CONFIG_PATH), { recursive: true });
  writeFileSync(CONFIG_PATH, JSON.stringify(cfg, null, 2));
}

function ensurePluginsShape(cfg) {
  if (!cfg.plugins) cfg.plugins = {};
  if (!Array.isArray(cfg.plugins.dirs)) cfg.plugins.dirs = [];
  if (!cfg.plugins.enabledPlugins) cfg.plugins.enabledPlugins = {};
  return cfg;
}

async function main() {
  // Silent no-op during local dev (postinstall fires on `npm install` in dev too)
  if (isSelfDev && !FORCE) {
    console.log("[cerebro-zcode] postinstall skipped (dev mode). Run `node install.js` to self-install.");
    return;
  }

  // Silent no-op if ZCode directory doesn't exist at all (user hasn't installed ZCode)
  if (!existsSync(ZCODE_DIR)) {
    console.log(`[cerebro-zcode] ZCode not found at ${ZCODE_DIR} — skipping auto-install.`);
    console.log("[cerebro-zcode] Once ZCode is installed, re-run: npx cerebro-zcode-install");
    return;
  }

  console.log("[cerebro-zcode] Installing plugin into ZCode...");

  // 1. Copy plugin assets to stable location (zero-dependency — no npm install)
  mkdirSync(TARGET, { recursive: true });
  cpSync(PKG_ROOT, TARGET, {
    recursive: true,
    filter: (src) => {
      const rel = src.slice(PKG_ROOT.length).replace(/\\/g, "/");
      if (rel.includes("/node_modules/")) return false;
      if (rel.includes("/.git/")) return false;
      if (rel.endsWith(".tgz")) return false;
      return true;
    },
  });

  // 2. Register in config.json plugins.dirs; clean up legacy global mcp.servers
  //    (pre-0.3.0 registered MCP globally; v0.3.0 loads it from .mcp.json)
  const cfg = ensurePluginsShape(readConfig());
  const targetNorm = TARGET.replace(/\//g, "\\");
  if (!cfg.plugins.dirs.some((d) => resolve(String(d)) === resolve(TARGET))) {
    cfg.plugins.dirs.push(targetNorm);
  }
  if (cfg.mcp?.servers?.cerebro) {
    delete cfg.mcp.servers.cerebro;
    console.log("[cerebro-zcode] ✓ Removed legacy cerebro entry from mcp.servers");
  }
  writeConfig(cfg);

  console.log(`[cerebro-zcode] ✓ Plugin copied to ${TARGET} (zero-dependency)`);
  console.log(`[cerebro-zcode] ✓ Registered plugins.dirs in ${CONFIG_PATH}`);
  console.log("");
  console.log("[cerebro-zcode] Next steps:");
  console.log("  1. Restart ZCode (close all windows and reopen).");
  console.log("  2. Start a new session — [CEREBRO-MEMORY] is injected at session start.");
  console.log("  3. Set OMEM_API_KEY env var (or ~/.config/cerebro/config.json) to enable memory.");
}

main().catch((err) => {
  // postinstall failures must not break npm install — warn and continue
  console.warn(`[cerebro-zcode] postinstall warning: ${err instanceof Error ? err.message : String(err)}`);
  console.warn("[cerebro-zcode] You can install manually later with: npx cerebro-zcode-install");
});
