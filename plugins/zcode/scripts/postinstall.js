#!/usr/bin/env node
// postinstall hook — runs automatically after `npm install @ourmem/zcode`.
// Copies the plugin assets to ~/.zcode/plugins/cerebro/ and registers the path
// in ~/.zcode/cli/config.json (plugins.dirs). After this, restarting ZCode loads
// the plugin automatically (source:"inline", defaultEnabled:true).
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

  // 1. Copy plugin assets to stable location.
  // node_modules is NOT copied (it's hoisted to the npm root, not inside the pkg).
  // We install mcp runtime deps into TARGET separately below to avoid recursion.
  mkdirSync(TARGET, { recursive: true });
  cpSync(PKG_ROOT, TARGET, {
    recursive: true,
    filter: (src) => {
      const rel = src.slice(PKG_ROOT.length).replace(/\\/g, "/");
      if (rel.includes("/node_modules/")) return false;
      if (rel.includes("/.git/")) return false;
      return true;
    },
  });

  // 1b. Install MCP runtime deps into TARGET so mcp/server.js can resolve
  // @modelcontextprotocol/sdk + zod. Skip our own postinstall to avoid recursion.
  try {
    const { spawnSync } = await import("node:child_process");
    const res = spawnSync(
      process.platform === "win32" ? "npm.cmd" : "npm",
      ["install", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund"],
      { cwd: TARGET, stdio: "pipe", encoding: "utf-8", shell: true },
    );
    if (res.status !== 0) {
      console.warn(`[cerebro-zcode] npm install in TARGET failed (exit ${res.status}). MCP tools may not work.`);
      if (res.stderr) console.warn(`[cerebro-zcode] ${res.stderr.slice(0, 300)}`);
    } else {
      console.log("[cerebro-zcode] ✓ MCP dependencies installed");
    }
  } catch (err) {
    console.warn(`[cerebro-zcode] Could not install MCP deps: ${err instanceof Error ? err.message : String(err)}`);
    console.warn("[cerebro-zcode] Run `npm install` manually in the plugin dir to enable MCP tools.");
  }

  // 2. Register in config.json: plugins.dirs (hooks/skills) + global mcp.servers (MCP tools)
  //    ZCode does NOT spawn MCP from inline (plugins.dirs) plugins — only global
  //    mcp.servers works. So we register cerebro's MCP server there too.
  const cfg = ensurePluginsShape(readConfig());
  const targetNorm = TARGET.replace(/\//g, "\\");
  if (!cfg.plugins.dirs.some((d) => resolve(String(d)) === resolve(TARGET))) {
    cfg.plugins.dirs.push(targetNorm);
  }
  if (!cfg.mcp) cfg.mcp = {};
  if (!cfg.mcp.servers) cfg.mcp.servers = {};
  const serverJsNorm = join(TARGET, "mcp", "server.js").replace(/\//g, "\\");
  cfg.mcp.servers.cerebro = { type: "stdio", command: "node", args: [serverJsNorm] };
  writeConfig(cfg);

  console.log(`[cerebro-zcode] ✓ Plugin copied to ${TARGET}`);
  console.log(`[cerebro-zcode] ✓ Registered plugins.dirs + mcp.servers in ${CONFIG_PATH}`);
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
