#!/usr/bin/env node
// Dev sync installer for the Cerebro ZCode plugin.
//
// The STANDARD installation path is the ZCode marketplace (or npm
// `@mingxy/cerebro-zcode`, whose postinstall wraps this logic). This script
// is for development: sync the repo checkout into ~/.zcode/plugins/cerebro
// and register it via plugins.dirs (source:"inline", auto-loaded on restart).
//
// v0.3.0: the plugin is fully zero-dependency (MCP server included), so no
// `npm install` step is needed after copying. MCP tools load via the plugin's
// own .mcp.json — no global mcp.servers registration either.
//
// Usage:
//   node install.js                          # install to ~/.zcode/plugins/cerebro
//   node install.js --target /custom/path    # install to custom path
//   node install.js --uninstall              # remove from config.json + delete files

import { cpSync, rmSync, existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PLUGIN_SRC = __dirname;
const ZCODE_DIR = join(homedir(), ".zcode");
const CONFIG_PATH = join(ZCODE_DIR, "cli", "config.json");
const DEFAULT_TARGET = join(homedir(), ".zcode", "plugins", "cerebro");

const args = process.argv.slice(2);
const uninstall = args.includes("--uninstall");
const targetIdx = args.indexOf("--target");
const target = targetIdx >= 0 && args[targetIdx + 1] ? resolve(args[targetIdx + 1]) : DEFAULT_TARGET;

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

if (uninstall) {
  const cfg = ensurePluginsShape(readConfig());
  const before = cfg.plugins.dirs.length;
  cfg.plugins.dirs = cfg.plugins.dirs.filter((d) => {
    const norm = String(d).replace(/\\/g, "/").toLowerCase();
    return !(norm.includes("/cerebro") && (norm.includes(".zcode/plugins") || norm === target.replace(/\\/g, "/").toLowerCase()));
  });
  // Clean up a legacy global mcp.servers registration (pre-0.3.0 installs)
  if (cfg.mcp?.servers?.cerebro) {
    delete cfg.mcp.servers.cerebro;
    console.log("✓ Removed legacy cerebro entry from mcp.servers");
  }
  // best-effort: remove any cerebro enabledPlugins keys
  for (const k of Object.keys(cfg.plugins.enabledPlugins)) {
    if (k.toLowerCase().includes("cerebro")) delete cfg.plugins.enabledPlugins[k];
  }
  writeConfig(cfg);
  if (existsSync(target)) {
    rmSync(target, { recursive: true, force: true });
    console.log(`✓ Removed plugin files: ${target}`);
  }
  console.log(`✓ Removed ${before - cfg.plugins.dirs.length} dir(s) from plugins.dirs`);
  console.log("\nRestart ZCode to complete uninstallation.");
  process.exit(0);
}

// ── Install ──────────────────────────────────────────────────────────
console.log("Cerebro ZCode Plugin Installer (dev sync)");
console.log("===========================================");

// 1. Copy plugin to target (zero-dependency: no npm install needed)
console.log(`\n[1/3] Copying plugin to: ${target}`);
mkdirSync(target, { recursive: true });
cpSync(PLUGIN_SRC, target, {
  recursive: true,
  filter: (src) => {
    const rel = src.slice(PLUGIN_SRC.length).replace(/\\/g, "/");
    if (rel.includes("/node_modules/")) return false;
    if (rel.includes("/.git/")) return false;
    if (rel.endsWith(".tgz")) return false;
    return true;
  },
});
console.log("   ✓ Copied (zero-dependency — no npm install required).");

// 2. Register in config.json plugins.dirs (hooks + skills + MCP auto-loaded)
console.log(`\n[2/3] Registering plugin dir in: ${CONFIG_PATH}`);
const cfg = ensurePluginsShape(readConfig());
const targetNorm = target.replace(/\//g, "\\"); // keep native sep on Windows
if (!cfg.plugins.dirs.some((d) => resolve(String(d)) === resolve(target))) {
  cfg.plugins.dirs.push(targetNorm);
  console.log(`   ✓ Added to plugins.dirs: ${targetNorm}`);
} else {
  console.log("   ✓ Already in plugins.dirs (no change).");
}

// 2b. Clean up a legacy global mcp.servers registration (pre-0.3.0 installs
//     registered MCP globally; v0.3.0 loads it from the plugin's .mcp.json,
//     and keeping both would spawn two identical MCP servers)
if (cfg.mcp?.servers?.cerebro) {
  delete cfg.mcp.servers.cerebro;
  console.log("   ✓ Removed legacy cerebro entry from mcp.servers (plugin .mcp.json takes over).");
}
writeConfig(cfg);
console.log("   ✓ config.json saved.");

// 3. Check API key
console.log("\n[3/3] Checking credentials...");
if (process.env.OMEM_API_KEY) {
  console.log("   ✓ OMEM_API_KEY is set in environment.");
} else {
  console.log("   ⚠ OMEM_API_KEY not set in current environment.");
  console.log("     The plugin will show a guidance message until you set it.");
  console.log("     Set it via env var, or in ~/.config/cerebro/config.json:");
  console.log('       { "connection": { "apiKey": "your-key", "apiUrl": "https://www.mengxy.cc" } }');
}

console.log("\n===========================================");
console.log("✓ Installation complete!");
console.log("\nNext steps:");
console.log("  1. Restart ZCode (close all windows and reopen).");
console.log("  2. Start a new session — you should see [CEREBRO-MEMORY] injected");
console.log("     with the time header, user profile + project memories.");
console.log("  3. Set OMEM_API_KEY if not done yet.");
console.log(`\nPlugin location: ${target}`);
console.log(`Logs: ~/.config/cerebro/logs/cerebro-zcode.log`);
