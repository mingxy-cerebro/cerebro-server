#!/usr/bin/env node
// preuninstall hook — runs automatically before `npm uninstall @ourmem/zcode`.
// Removes the plugin from ~/.zcode/plugins/cerebro/ and unregisters the path
// from config.json plugins.dirs.

import { rmSync, existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname, resolve } from "node:path";

const ZCODE_DIR = join(homedir(), ".zcode");
const CONFIG_PATH = join(ZCODE_DIR, "cli", "config.json");
const TARGET = join(homedir(), ".zcode", "plugins", "cerebro");

function readConfig() {
  try {
    return JSON.parse(readFileSync(CONFIG_PATH, "utf-8"));
  } catch {
    return {};
  }
}

function writeConfig(cfg) {
  try {
    mkdirSync(dirname(CONFIG_PATH), { recursive: true });
    writeFileSync(CONFIG_PATH, JSON.stringify(cfg, null, 2));
  } catch {}
}

async function main() {
  if (!existsSync(ZCODE_DIR)) {
    // Nothing to clean — ZCode never installed
    return;
  }

  // 1. Unregister from config.json
  try {
    const cfg = readConfig();
    if (cfg.plugins && Array.isArray(cfg.plugins.dirs)) {
      const before = cfg.plugins.dirs.length;
      cfg.plugins.dirs = cfg.plugins.dirs.filter((d) => resolve(String(d)) !== resolve(TARGET));
      if (cfg.plugins.dirs.length !== before) {
        writeConfig(cfg);
        console.log(`[cerebro-zcode] ✓ Unregistered from ${CONFIG_PATH}`);
      }
    }
  } catch {}

  // 2. Remove plugin files
  if (existsSync(TARGET)) {
    try {
      rmSync(TARGET, { recursive: true, force: true });
      console.log(`[cerebro-zcode] ✓ Removed plugin files: ${TARGET}`);
    } catch (err) {
      console.warn(`[cerebro-zcode] Could not remove ${TARGET}: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  console.log("[cerebro-zcode] Uninstalled. Restart ZCode to complete removal.");
}

main().catch((err) => {
  // preuninstall failures must not break npm uninstall
  console.warn(`[cerebro-zcode] preuninstall warning: ${err instanceof Error ? err.message : String(err)}`);
});
