import type { Plugin } from "@opencode-ai/plugin";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
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

function showToast(tui: any, title: string, message: string, variant: string = "info", duration: number = 5000) {
  if (!tui) return;
  setTimeout(() => {
    try {
      tui.showToast({ body: { title, message, variant, duration } });
    } catch {}
  }, 3000);
}

const OmemPlugin: Plugin = async (input) => {
  const { directory, client } = input;
  const tui = (client as any)?.tui;

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
    showToast(
      tui,
      `🧠 Omem v${pluginVersion} · Connected`,
      `${config.apiUrl.replace(/^https?:\/\//, "")}`,
      "success",
      6000
    );
    logInfo(`Connected to ${config.apiUrl}`);
  } catch (err) {
    const errMsg = err instanceof Error ? err.message : String(err);
    logError(`Connection failed: ${errMsg}`);
    if (errMsg.includes("[omem]")) {
      const cleanMsg = errMsg.replace(/^\[omem\]\s*/, "");
      showToast(
        tui,
        `🧠 Omem v${pluginVersion} · Server Error`,
        cleanMsg.substring(0, 150),
        "error",
        8000
      );
    } else {
      showToast(
        tui,
        `🧠 Omem v${pluginVersion} · Connection Failed`,
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
    "experimental.chat.system.transform": async (input: any, output: any) => {
      if (input.sessionID) currentSessionId = input.sessionID;
      return recallHook(input, output);
    },
    "chat.message": keywordDetectionHook(omemClient, containerTags, config.autoCaptureThreshold, tui, config.ingestMode),
    "experimental.session.compacting": compactingHook(omemClient, containerTags, tui, config.ingestMode),
    tool: buildTools(omemClient, containerTags, { agentId, getSessionId: () => currentSessionId }),
    event: sessionIdleHook(omemClient, containerTags, tui, config.ingestMode, config.autoCaptureThreshold),
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
