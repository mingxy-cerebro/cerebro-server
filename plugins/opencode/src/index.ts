import type { Plugin } from "@opencode-ai/plugin";
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { CerebroClient } from "./client.js";
import { chatMessageRecallHook, autocontinueHook, compactingHook, sessionIdleHook, sessionMessages, firstMessages, showToast, timeMemorySystemHook } from "./hooks.js";
import { detectSaveKeyword, detectRecallKeyword, KEYWORD_NUDGE, RECALL_NUDGE } from "./keywords.js";
import { getUserTag, getProjectTag } from "./tags.js";
import { buildTools } from "./tools.js";
import { logInfo, logDebug, logError, setOpencodeClient } from "./logger.js";
import { loadPluginConfig, resolveAgentPolicy } from "./config.js";
import { checkAndUpdate } from "./updater.js";
import { startWebServer, stopWebServer, type WebServerHandle } from "./web-server.js";

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
  const cached = autoStoreSessions.get(sessionId);
  if (cached !== undefined) return cached;
  // Fallback: read from persisted file (survives restart)
  try {
    const data = JSON.parse(readFileSync(getStateFilePath(sessionId), "utf-8"));
    const enabled = data.enabled ?? true;
    autoStoreSessions.set(sessionId, enabled); // cache for next time
    return enabled;
  } catch {
    return true; // file doesn't exist → default ON
  }
}

export function setAutoStoreEnabled(sessionId: string, enabled: boolean): void {
  autoStoreSessions.set(sessionId, enabled);
  try {
    writeFileSync(getStateFilePath(sessionId), JSON.stringify({ enabled }));
  } catch {}
}

(globalThis as any).__cerebro_autoStoreMap = autoStoreSessions;

const OmemPlugin: Plugin = async (input) => {
  const { directory, client } = input;
  // Proxy: dynamically resolve client.tui on each access so toast works
  // even if client.tui isn't ready yet at plugin init time
  const tui = new Proxy({} as any, {
    get(_, prop) {
      const realTui = (client as any)?.tui;
      const val = realTui?.[prop];
      return typeof val === "function" ? val.bind(realTui) : val;
    },
  });

  // Load overrides from opencode.json plugin_config
  let overrides: Record<string, unknown> = {};
  try {
    const ocCfg = JSON.parse(readFileSync(join(directory, "opencode.json"), "utf-8"));
    const pc = ocCfg?.plugin_config?.["@mingxy/cerebro"];
    if (pc) overrides = pc;
  } catch {}

  const config = loadPluginConfig(overrides as any);
  const STARTUP_DELAY = 5000;

  setOpencodeClient(client);

  const cerebroClient = new CerebroClient(config.connection.apiUrl, config.connection.apiKey, config);

  try {
    await cerebroClient.getStats();
    logInfo(`Connected to ${config.connection.apiUrl}`);
  } catch (err) {
    const errMsg = err instanceof Error ? err.message : String(err);
    logError(`Connection failed: ${errMsg}`);
    if (errMsg.includes("[cerebro]")) {
      const cleanMsg = errMsg.replace(/^\[cerebro\]\s*/, "");
      showToast(
        tui,
        `🧠 Cerebro v${pluginVersion} · Server Error`,
        cleanMsg.substring(0, 150),
        "error",
        STARTUP_DELAY
      );
    } else {
      showToast(
        tui,
        `🧠 Cerebro v${pluginVersion} · Connection Failed`,
        `Unable to reach ${config.connection.apiUrl}`,
        "error",
        STARTUP_DELAY
      );
    }
  }

  const email = process.env.GIT_AUTHOR_EMAIL || process.env.USER || "unknown";
  const cwd = directory || process.cwd();
  const containerTags = [getUserTag(email), getProjectTag(cwd)];
  const agentId = process.env.OMEM_AGENT_ID || "opencode";

  let mainSessionId: string | undefined;
  let mainSessionLocked = false;
  let cachedAgentName: string | undefined;

  const chatMessageRecall = chatMessageRecallHook(cerebroClient, containerTags, tui, config, () => cachedAgentName || agentId, directory);

  let webServer: WebServerHandle | null = null;
  const webEnabled = config.web?.enabled !== false;
  let webPort: number | undefined;
  if (webEnabled) {
    try {
      webServer = await startWebServer({
        apiUrl: config.connection.apiUrl,
        port: config.web?.port,
      });
      if (webServer) {
        const addr = webServer.address();
        webPort = typeof addr === "object" && addr ? addr.port : config.web?.port || 5212;
        logInfo(`Web UI available at http://localhost:${webPort}`);
      }
    } catch (err) {
      logError(`Web server start failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  if (webPort) {
    showToast(tui, `🧠 Cerebro Connected · v${pluginVersion}`, `🌐 Open in browser http://localhost:${webPort}`, "success", STARTUP_DELAY);
  } else {
    showToast(tui, `🧠 Cerebro Connected · v${pluginVersion}`, "No web server", "success", STARTUP_DELAY);
  }

  // Auto-update check (fire-and-forget, non-blocking)
  checkAndUpdate(tui, pluginVersion).catch(() => {});

  const shutdown = async () => {
    try {
      if (webServer) {
        await stopWebServer(webServer);
        webServer = null;
      }
    } catch {}
    process.exit(0);  // 强制退出，确保 HTTP server 停止
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);
  process.on("disconnect", shutdown);  // OpenCode 窗口关闭时触发

  return {
    config: async (cfg: any) => {
      cfg.command ??= {};
      cfg.command["memory-toggle"] = {
        template: "Use the memory_toggle tool with state='$ARGUMENTS' to toggle Cerebro auto-store for this session. You MUST call the memory_toggle tool, do not just acknowledge.",
        description: "Toggle Cerebro auto-store ON or OFF for current session",
      };
    },
    "chat.message": async (input: any, output: any) => {
      if (input.sessionID && !mainSessionLocked) {
        mainSessionId = input.sessionID;
        mainSessionLocked = true;
        logInfo("mainSessionId locked", { sessionId: input.sessionID });
      }
      await chatMessageRecall(input, output);
      const textContent = output.parts
        .filter((p: any) => p.type === "text" && !(p as any).synthetic)
        .map((p: any) => p.text || (p as any).content || "")
        .join(" ")
        || (output.message as any).content
        || "";
      if (!firstMessages.has(input.sessionID)) {
        firstMessages.set(input.sessionID, textContent);
      }
      if (detectSaveKeyword(textContent)) {
        output.parts.push({
          id: `prt_cerebro-save-${Date.now()}`,
          sessionID: input.sessionID,
          messageID: output.message?.id,
          type: "text",
          text: KEYWORD_NUDGE,
          synthetic: true,
        } as any);
        logDebug("save keyword detected, nudge pushed", { sessionId: input.sessionID });
      }
      if (detectRecallKeyword(textContent)) {
        output.parts.push({
          id: `prt_cerebro-recall-${Date.now()}`,
          sessionID: input.sessionID,
          messageID: output.message?.id,
          type: "text",
          text: RECALL_NUDGE,
          synthetic: true,
        } as any);
        logDebug("recall keyword detected, nudge pushed", { sessionId: input.sessionID });
      }
      const policy = resolveAgentPolicy(agentId, config);
      if (policy !== "none") {
        if (!sessionMessages.has(input.sessionID)) {
          sessionMessages.set(input.sessionID, []);
        }
        sessionMessages.get(input.sessionID)!.push({ role: "user", content: textContent });
      }
    },
    "experimental.session.compacting": compactingHook(cerebroClient, containerTags, tui, config.ingest.ingestMode, isAutoStoreEnabled, () => mainSessionId, client, config, agentId, directory),
    "experimental.compaction.autocontinue": autocontinueHook(cerebroClient, containerTags, tui, config.ingest.ingestMode, isAutoStoreEnabled, () => mainSessionId, client, config, agentId, directory),
    tool: buildTools(cerebroClient, containerTags, { agentId, getSessionId: () => mainSessionId, getAgentName: () => cachedAgentName || agentId, getProjectPath: () => directory, config }),
    event: sessionIdleHook(cerebroClient, containerTags, tui, client, config.ingest.ingestMode, config.ingest.autoCaptureThreshold, () => mainSessionId, isAutoStoreEnabled, agentId, config, (name: string) => { cachedAgentName = name; }, directory),
    "shell.env": async (_input: any, output: any) => {
      if (directory) {
        output.env.OMEM_PROJECT_DIR = directory;
      }
    },
    "experimental.chat.system.transform": timeMemorySystemHook(),
  };
};

export { OmemPlugin };

export default {
  id: "ourmem",
  server: OmemPlugin,
};
