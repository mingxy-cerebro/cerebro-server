import type { Plugin } from "@opencode-ai/plugin";
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { OmemClient } from "./client.js";
import { autoRecallHook, compactingHook, keywordDetectionHook, sessionIdleHook } from "./hooks.js";
import { getUserTag, getProjectTag } from "./tags.js";
import { buildTools } from "./tools.js";
import { logInfo, logError } from "./logger.js";
import { loadPluginConfig } from "./config.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

let pluginVersion = "unknown";
try {
  const pkg = JSON.parse(readFileSync(join(__dirname, "..", "package.json"), "utf-8"));
  if (pkg?.version && typeof pkg.version === "string") {
    pluginVersion = pkg.version;
  }
} catch {}

// Per-session auto-store toggle: sessionId → enabled (default: true = auto-store on)
const autoStoreSessions = new Map<string, boolean>();

function getStateFilePath(sessionId: string): string {
  return join(tmpdir(), `cerebro_autostore_${sessionId}.json`);
}

export function isAutoStoreEnabled(sessionId: string | undefined): boolean {
  if (!sessionId) return true;
  return autoStoreSessions.get(sessionId) ?? true;
}

export function setAutoStoreEnabled(sessionId: string, enabled: boolean): void {
  autoStoreSessions.set(sessionId, enabled);
  try {
    writeFileSync(getStateFilePath(sessionId), JSON.stringify({ enabled }));
  } catch {}
}

(globalThis as any).__cerebro_autoStoreMap = autoStoreSessions;

function showToast(tui: any, title: string, message?: string, variant: string = "info", duration: number = 5000) {
  if (!tui) return;
  setTimeout(() => {
    try {
      const body: any = { variant, duration };
      if (message) {
        body.title = title;
        body.message = message;
      } else {
        body.message = title;
      }
      tui.showToast({ body });
    } catch (err) {
      console.error("[cerebro] showToast failed:", err);
    }
  }, 3000);
}

const OmemPlugin: Plugin = async (input) => {
  const { directory, client } = input;
  // Proxy: dynamically resolve client.tui on each access so toast works
  // even if client.tui isn't ready yet at plugin init time
  const tui = new Proxy({} as any, {
    get(_, prop) {
      return (client as any)?.tui?.[prop];
    },
  });

  // Load overrides from opencode.json plugin_config
  let overrides: Record<string, unknown> = {};
  try {
    const ocCfg = JSON.parse(readFileSync(join(directory, "opencode.json"), "utf-8"));
    const pc = ocCfg?.plugin_config?.["@mingxy/omem"] || ocCfg?.plugin_config?.["@ourmem/opencode"];
    if (pc) overrides = pc;
  } catch {}

  const config = loadPluginConfig(overrides as any);

  const omemClient = new OmemClient(config.apiUrl, config.apiKey, config);

  // 启动时检测连接状态
  try {
    await omemClient.getStats();
    showToast(tui, "🧠 Cerebro · Connected", `Version v${pluginVersion}`, "success", 6000);
    logInfo(`Connected to ${config.apiUrl}`);
  } catch (err) {
    const errMsg = err instanceof Error ? err.message : String(err);
    logError(`Connection failed: ${errMsg}`);
    if (errMsg.includes("[omem]")) {
      const cleanMsg = errMsg.replace(/^\[omem\]\s*/, "");
      showToast(
        tui,
        `🧠 Cerebro v${pluginVersion} · Server Error`,
        cleanMsg.substring(0, 150),
        "error",
        8000
      );
    } else {
      showToast(
        tui,
        `🧠 Cerebro v${pluginVersion} · Connection Failed`,
        `Unable to reach ${config.apiUrl}`,
        "error",
        8000
      );
    }
  }

  const email = process.env.GIT_AUTHOR_EMAIL || process.env.USER || "unknown";
  const cwd = directory || process.cwd();
  const containerTags = [getUserTag(email), getProjectTag(cwd)];
  const agentId = process.env.OMEM_AGENT_ID || "opencode";

  let currentSessionId: string | undefined;

  const recallHook = autoRecallHook(omemClient, containerTags, tui, config);

  return {
    config: async (cfg: any) => {
      cfg.command ??= {};
      cfg.command["memory-toggle"] = {
        template: "Use the memory_toggle tool with state='$ARGUMENTS' to toggle Cerebro auto-store for this session. You MUST call the memory_toggle tool, do not just acknowledge.",
        description: "Toggle Cerebro auto-store ON or OFF for current session",
      };
    },
    "experimental.chat.system.transform": async (input: any, output: any) => {
      if (input.sessionID) currentSessionId = input.sessionID;
      return recallHook(input, output);
    },
    "chat.message": keywordDetectionHook(omemClient, containerTags, config.autoCaptureThreshold, tui, config.ingestMode),
    "experimental.session.compacting": compactingHook(omemClient, containerTags, tui, config.ingestMode, isAutoStoreEnabled),
    tool: buildTools(omemClient, containerTags, { agentId, getSessionId: () => currentSessionId }),
    event: sessionIdleHook(omemClient, containerTags, tui, client, config.ingestMode, config.autoCaptureThreshold, () => currentSessionId, isAutoStoreEnabled),
    "shell.env": async (_input: any, output: any) => {
      if (directory) {
        output.env.OMEM_PROJECT_DIR = directory;
      }
    },
  };
};

export { OmemPlugin };

export default {
  id: "ourmem",
  server: OmemPlugin,
};
