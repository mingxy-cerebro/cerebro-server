#!/usr/bin/env node
// One-shot installer for the Cerebro ZCode plugin.
// Copies this plugin to a stable location and registers it in ZCode's config.json
// via plugins.dirs (source:"inline" — auto-loaded on restart, no marketplace edit needed).
//
// Usage:
//   node install.js                          # install to ~/.zcode/plugins/cerebro
//   node install.js --target /custom/path    # install to custom path
//   node install.js --uninstall              # remove from config.json + delete files
//
// After install: restart ZCode. The plugin's SessionStart/Stop/PreCompact hooks
// fire automatically. Set OMEM_API_KEY env var (or ~/.config/cerebro/config.json) to enable.

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
  // Remove cerebro from global mcp.servers
  if (cfg.mcp?.servers?.cerebro) {
    delete cfg.mcp.servers.cerebro;
    console.log("✓ Removed cerebro from mcp.servers");
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
console.log("Cerebro ZCode Plugin Installer");
console.log("================================");

// 1. Copy plugin to target (excluding node_modules; deps installed separately)
console.log(`\n[1/4] Copying plugin to: ${target}`);
mkdirSync(target, { recursive: true });
cpSync(PLUGIN_SRC, target, {
  recursive: true,
  filter: (src) => {
    const rel = src.slice(PLUGIN_SRC.length).replace(/\\/g, "/");
    if (rel.includes("/node_modules/")) return false;
    if (rel.includes("/.git/")) return false;
    return true;
  },
});
console.log("   ✓ Copied.");

// 1b. Install MCP runtime deps into target (for mcp/server.js)
console.log(`\n[2/4] Installing MCP dependencies (this may take a moment)...`);
try {
  const { spawnSync } = await import("node:child_process");
  const res = spawnSync(
    process.platform === "win32" ? "npm.cmd" : "npm",
    ["install", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund"],
    { cwd: target, stdio: "inherit", shell: true },
  );
  if (res.status !== 0) {
    console.warn("   ⚠ npm install failed — MCP tools may not work until you run `npm install` manually.");
  } else {
    console.log("   ✓ MCP dependencies installed (@modelcontextprotocol/sdk, zod).");
  }
} catch (err) {
  console.warn(`   ⚠ Could not install deps: ${err instanceof Error ? err.message : String(err)}`);
}

// 3. Register in config.json plugins.dirs (hooks + skills auto-loaded)
console.log(`\n[3/5] Registering plugin dir in: ${CONFIG_PATH}`);
const cfg = ensurePluginsShape(readConfig());
const targetNorm = target.replace(/\//g, "\\"); // keep native sep on Windows
if (!cfg.plugins.dirs.some((d) => resolve(String(d)) === resolve(target))) {
  cfg.plugins.dirs.push(targetNorm);
  console.log(`   ✓ Added to plugins.dirs: ${targetNorm}`);
} else {
  console.log("   ✓ Already in plugins.dirs (no change).");
}

// 4. Register MCP server in global mcp.servers
//    NOTE: ZCode does NOT load MCP from inline (plugins.dirs) plugins — the
//    server.js is never spawned. The ONLY working path is global mcp.servers
//    (verified by spawn-probe: inline plugin MCP gets 0 spawns, global gets
//    spawned immediately). So we register cerebro's MCP server here.
console.log(`\n[4/5] Registering MCP server in global mcp.servers...`);
if (!cfg.mcp) cfg.mcp = {};
if (!cfg.mcp.servers) cfg.mcp.servers = {};
const serverJsNorm = join(target, "mcp", "server.js").replace(/\//g, "\\");
if (!cfg.mcp.servers.cerebro) {
  cfg.mcp.servers.cerebro = {
    type: "stdio",
    command: "node",
    args: [serverJsNorm],
  };
  console.log(`   ✓ Added cerebro to mcp.servers → ${serverJsNorm}`);
} else {
  // Update args to point at current install location
  cfg.mcp.servers.cerebro.args = [serverJsNorm];
  console.log(`   ✓ Updated cerebro mcp.servers args → ${serverJsNorm}`);
}
writeConfig(cfg);
console.log("   ✓ config.json saved.");

// 5. Check API key
console.log("\n[5/5] Checking credentials...");
if (process.env.OMEM_API_KEY) {
  console.log("   ✓ OMEM_API_KEY is set in environment.");
} else {
  console.log("   ⚠ OMEM_API_KEY not set in current environment.");
  console.log("     The plugin will show a guidance message until you set it.");
  console.log("     Set it via env var, or in ~/.config/cerebro/config.json:");
  console.log('       { "connection": { "apiKey": "your-key", "apiUrl": "https://www.mengxy.cc" } }');
}

console.log("\n================================");
console.log("✓ Installation complete!");
console.log("\nNext steps:");
console.log("  1. Restart ZCode (close all windows and reopen).");
console.log("  2. Start a new session — you should see [CEREBRO-MEMORY] injected");
console.log("     with your user profile + project memories at session start.");
console.log("  3. Set OMEM_API_KEY if not done yet.");
console.log(`\nPlugin location: ${target}`);
console.log(`Logs: ~/.config/cerebro/logs/cerebro-zcode.log`);
