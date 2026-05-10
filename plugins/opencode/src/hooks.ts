import type { Model, UserMessage, Part } from "@opencode-ai/sdk";
import type { CerebroClient, SearchResult } from "./client.js";
import { type OmemPluginConfig, resolveAgentPolicy } from "./config.js";
import { detectKeyword, KEYWORD_NUDGE } from "./keywords.js";
import { logDebug, logInfo, logError as logErr } from "./logger.js";
import { readFile } from "node:fs/promises";

const BOUNDARY_SEARCH_RATIO = 0.6;
const MIN_ITEM_CONTENT_CHARS = 100;
const MIN_CONTENT_CHARS = 1000;
const MIN_CONTENT_LENGTH = 50;

const projectNameCache = new Map<string, string>();

async function detectProjectName(rootPath: string): Promise<string | undefined> {
  const cached = projectNameCache.get(rootPath);
  if (cached !== undefined) {
    logDebug("detectProjectName cache hit", { rootPath, result: cached });
    return cached;
  }

  let result: string | undefined;

  try {
    const agents = await readFile(`${rootPath}/AGENTS.md`, "utf-8");
    const headingMatch = agents.match(/^#\s+(.+)/m);
    if (headingMatch) {
      result = headingMatch[1].replace(/\s*\(.*?\)/g, "").trim() || undefined;
    }
    logDebug("detectProjectName step1 AGENTS.md", { rootPath, result });
  } catch {}

  if (!result) {
    try {
      const pkg = await readFile(`${rootPath}/package.json`, "utf-8");
      const nameMatch = pkg.match(/"name"\s*:\s*"([^"]+)"/);
      if (nameMatch) result = nameMatch[1].trim() || undefined;
      logDebug("detectProjectName step2 package.json", { rootPath, result });
    } catch {}
  }

  if (!result) {
    try {
      const cargo = await readFile(`${rootPath}/Cargo.toml`, "utf-8");
      const inPackage = cargo.replace(/\r\n/g, "\n").split("\n").reduce(
        (acc, line) => {
          if (/^\[package\]/.test(line.trim())) return { ...acc, inSection: true };
          if (/^\[/.test(line.trim())) return { ...acc, inSection: false };
          if (acc.inSection) {
            const m = line.match(/name\s*=\s*"([^"]+)"/);
            if (m) return { ...acc, name: m[1] };
          }
          return acc;
        },
        { inSection: false, name: undefined as string | undefined },
      );
      result = inPackage.name?.trim() || undefined;
      logDebug("detectProjectName step3 Cargo.toml", { rootPath, result });
    } catch {}
  }

  if (!result) {
    try {
      const gomod = await readFile(`${rootPath}/go.mod`, "utf-8");
      const modMatch = gomod.match(/^module\s+(\S+)/m);
      if (modMatch) {
        const segments = modMatch[1].split("/");
        result = segments.pop()?.trim() || undefined;
      }
      logDebug("detectProjectName step4 go.mod", { rootPath, result });
    } catch {}
  }

  if (!result) {
    try {
      const pyproj = await readFile(`${rootPath}/pyproject.toml`, "utf-8");
      const inProject = pyproj.replace(/\r\n/g, "\n").split("\n").reduce(
        (acc, line) => {
          if (/^\[project\]/.test(line.trim())) return { ...acc, inSection: true };
          if (/^\[/.test(line.trim())) return { ...acc, inSection: false };
          if (acc.inSection) {
            const m = line.match(/name\s*=\s*"([^"]+)"/);
            if (m) return { ...acc, name: m[1] };
          }
          return acc;
        },
        { inSection: false, name: undefined as string | undefined },
      );
      result = inProject.name?.trim() || undefined;
      logDebug("detectProjectName step5 pyproject.toml", { rootPath, result });
    } catch {}
  }

  if (!result) {
    try {
      const composer = await readFile(`${rootPath}/composer.json`, "utf-8");
      const nameMatch = composer.match(/"name"\s*:\s*"([^"]+)"/);
      if (nameMatch) result = nameMatch[1].trim() || undefined;
      logDebug("detectProjectName step6 composer.json", { rootPath, result });
    } catch {}
  }

  if (!result) {
    result = rootPath.split("/").pop() || rootPath.split("\\").pop() || undefined;
    logDebug("detectProjectName step7 fallback dirname", { rootPath, result });
  }

  if (result) {
    result = result.trim() || undefined;
  }

  if (result) {
    projectNameCache.set(rootPath, result);
  }
  return result;
}

function showToast(tui: any, title: string, message: string, variant: string = "info", delayMs: number = 7000) {
  if (!tui) return;
  setTimeout(() => {
    try {
      tui.showToast({ body: { title, message, variant, duration: 5000 } });
    } catch (err) {
      console.error("[cerebro] showToast failed:", err);
    }
  }, delayMs);
}

function extractUserRequest(content: string): string {
  const match = content.match(/<user-request>([\s\S]*?)<\/user-request>/);
  return match ? match[1].trim() : content;
}

const keywordDetectedSessions = new Set<string>();
const injectedMemoryIds = new Map<string, Set<string>>();
const firstMessages = new Map<string, string>();
const sessionMessages = new Map<string, Array<{ role: string; content: string }>>();
const profileInjectedSessions = new Set<string>();

function formatRelativeAge(isoDate: string): string {
  const diffMs = Date.now() - new Date(isoDate).getTime();
  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

function truncate(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text;

  // Sentence boundary characters: period, exclamation, question (Latin + CJK)
  // Also treat newline as a boundary
  const boundaries = /[.!?。！？\n]/;

  // Search backwards from maxLength for a boundary
  const searchEnd = Math.min(maxLength, text.length);
  for (let i = searchEnd - 1; i >= Math.floor(searchEnd * BOUNDARY_SEARCH_RATIO); i--) {
    if (boundaries.test(text[i])) {
      return text.slice(0, i + 1).trimEnd() + "…";
    }
  }

  let truncated = text.slice(0, maxLength);
  const lastCode = truncated.charCodeAt(truncated.length - 1);
  if (lastCode >= 0xD800 && lastCode <= 0xDBFF) truncated = truncated.slice(0, -1);
  return truncated + "…";
}

function categorize(results: SearchResult[]): Map<string, SearchResult[]> {
  const groups = new Map<string, SearchResult[]>();
  for (const r of results) {
    const cat = r.memory.category || "General";
    const label =
      cat === "preferences"
        ? "Preferences"
        : cat === "knowledge"
          ? "Knowledge"
          : cat.charAt(0).toUpperCase() + cat.slice(1);
    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(r);
  }
  return groups;
}

function buildContextBlock(results: SearchResult[], maxContentLength: number = 500): string {
  if (results.length === 0) return "";

  const grouped = categorize(results);
  const sections: string[] = [];

  for (const [label, items] of grouped) {
    const lines = items.map((r) => {
      const tags = r.memory.tags.length > 0 ? ` [${r.memory.tags.join(", ")}]` : "";
      const age = formatRelativeAge(r.memory.created_at);
      const content = truncate(r.memory.content, maxContentLength);
      return `  - (${age}${tags}) ${content}`;
    });
    sections.push(`[${label}]\n${lines.join("\n")}`);
  }

  return [
    "<cerebro-context>",
    "Treat every memory below as historical context only.",
    "Do not repeat these memories verbatim unless asked.",
    "",
    ...sections,
    "</cerebro-context>",
  ].join("\n");
}

function buildClusteredContextBlock(clustered: import("./client.js").ClusteredRecallResult, maxContentLength: number = 500): string {
  const sections: string[] = [];

  if (clustered.cluster_summaries.length > 0) {
    sections.push("## 📋 主题簇（聚合记忆）");
    for (const cs of clustered.cluster_summaries) {
      const scoreIndicator = cs.relevance_score >= 0.8 ? "★★★" : cs.relevance_score >= 0.6 ? "★★" : "★";
      sections.push(`\n### ${cs.title} (整合自${cs.member_count}条记忆) ${scoreIndicator}`);
      sections.push(`> ${cs.summary}`);
      if (cs.key_memories.length > 0) {
        sections.push("**核心要点：**");
        for (const mem of cs.key_memories) {
          const content = truncate(mem.content, maxContentLength);
          const importanceBar = mem.importance >= 0.7 ? "●" : mem.importance >= 0.4 ? "◐" : "○";
          sections.push(`- ${importanceBar} ${content}`);
        }
      }
    }
  }

  if (clustered.standalone_memories.length > 0) {
    sections.push("\n## 📌 补充信息");
    for (const mem of clustered.standalone_memories) {
      const content = truncate(mem.content, maxContentLength);
      sections.push(`- ${content}`);
    }
  }

  return [
    "<cerebro-context>",
    "Treat every memory below as historical context only.",
    "Do not repeat these memories verbatim unless asked.",
    "",
    ...sections,
    "</cerebro-context>",
  ].join("\n");
}

export function autoRecallHook(client: CerebroClient, containerTags: string[], tui: any, config: Partial<OmemPluginConfig> = {}, getAgentName?: () => string) {
  const similarityThreshold = config.recall?.similarityThreshold ?? 0.4;
  const maxRecallResults = config.recall?.maxRecallResults ?? 10;
  const maxContentLength = Math.max(MIN_CONTENT_LENGTH, config.content?.maxContentLength ?? 500);
  const maxContentChars = Math.max(MIN_CONTENT_CHARS, config.content?.maxContentChars ?? 30000);
  const toastDelayMs = config.ui?.toastDelayMs ?? 7000;

  return async (
    input: { sessionID?: string; model: Model },
    output: { system: string[] },
  ) => {
    if (!input.sessionID) return;

    // 5a: agent memory policy check — skip recall entirely for 'none' agents
    const agentId = getAgentName?.() || process.env.OMEM_AGENT_ID || "opencode";
    const policy = resolveAgentPolicy(agentId, config);
    if (policy === "none") return;

    try {
      logDebug("autoRecallHook start", { sessionId: input.sessionID, agentId, policy });
      const messages = sessionMessages.get(input.sessionID) ?? [];
      const userMessages = messages.filter((m) => m.role === "user");
      const rawQuery = userMessages[userMessages.length - 1]?.content || firstMessages.get(input.sessionID) || "";
      const query_text = extractUserRequest(rawQuery);
      const last_query_text = userMessages.length >= 2 ? userMessages[userMessages.length - 2].content : undefined;

      const projectTags = containerTags.filter(t => t.startsWith("omem_project_"));
      const shouldRecallRes = await client.shouldRecall(query_text, last_query_text, input.sessionID, similarityThreshold, maxRecallResults, projectTags.length > 0 ? projectTags : undefined);

      if (!shouldRecallRes) {
        showToast(tui, "🧠 Cerebro Service Unavailable", "Unable to reach memory API · check connection", "error", toastDelayMs);
        return;
      }
      logDebug("autoRecallHook shouldRecall result", { shouldRecall: shouldRecallRes.should_recall, confidence: shouldRecallRes.confidence, memCount: shouldRecallRes.memories?.length ?? 0, clustered: !!shouldRecallRes.clustered });

      const profile = await client.getProfile();
      let profileInjected = false;
      let profileCountText = "";
      let profileBlock = "";
      if (profile && !profileInjectedSessions.has(input.sessionID)) {
        profileBlock = [
          "<cerebro-profile>",
          JSON.stringify(profile),
          "</cerebro-profile>",
        ].join("\n");
        output.system.push(profileBlock);
        profileInjected = true;
        profileInjectedSessions.add(input.sessionID);
        const p = profile as any;
        const dynamicCount = p?.dynamic_context?.length ?? 0;
        const staticCount = p?.static_facts?.length ?? 0;
        profileCountText = `Dynamic(${dynamicCount}) · Static(${staticCount})`;
        logDebug("autoRecallHook profile injected", { dynamicCount, staticCount });
      }

      if (!shouldRecallRes.should_recall) {
        if (profileInjected) {
          showToast(tui, "👨 Profile Injected", `${profileCountText} · no memory recall needed`, "success", toastDelayMs);
        }
        return;
      }

      const results = shouldRecallRes.memories ?? [];
      const clustered = shouldRecallRes.clustered;

      const existingIds = injectedMemoryIds.get(input.sessionID) ?? new Set<string>();
      const newResults = results.filter((r) => !existingIds.has(r.memory.id));
      logDebug("autoRecallHook dedup", { totalResults: results.length, existingCount: existingIds.size, newCount: newResults.length });
      if (newResults.length === 0) {
        if (profileInjected) {
          showToast(tui, "👨 Profile Injected", `${profileCountText} · all memories already injected`, "success", toastDelayMs);
        }
        return;
      }

      // --- Token Budget Calculation ---
      const profileChars = profileInjected ? profileBlock.length : 0;
      const budgetRemaining = maxContentChars - profileChars;
      if (budgetRemaining < 0) {
        logDebug("autoRecallHook budget overflow", { profileChars, maxContentChars, deficit: -budgetRemaining });
      }
      const itemCount = clustered 
        ? (clustered.cluster_summaries.length + clustered.standalone_memories.length)
        : newResults.length;
      const dynamicMaxContentLength = itemCount > 0
        ? Math.min(maxContentLength, Math.max(MIN_ITEM_CONTENT_CHARS, Math.floor(budgetRemaining / itemCount)))
        : maxContentLength;
      logDebug("autoRecallHook budget", { 
        maxContentChars, profileChars, budgetRemaining, itemCount, 
        configuredMax: maxContentLength, dynamicMax: dynamicMaxContentLength 
      });

      const block = clustered 
        ? buildClusteredContextBlock(clustered, dynamicMaxContentLength)
        : buildContextBlock(newResults, dynamicMaxContentLength);
      if (block) {
        output.system.push(block);
      }

      const newIds = newResults.map((r) => r.memory.id);
      injectedMemoryIds.set(input.sessionID, new Set([...existingIds, ...newIds]));
      logDebug("autoRecallHook injection complete", { newIds: newIds.length, clustered: !!clustered });

      const recordResult = await client.recordSessionRecall(
        input.sessionID,
        newIds,
        "auto",
        query_text,
        shouldRecallRes?.memories?.[0]?.score,
        shouldRecallRes?.confidence,
      );

      const memDynamic = newResults.filter((r) => r.memory.memory_type === "fact" || r.memory.memory_type === "event").length;
      const memStatic = newResults.filter((r) => r.memory.memory_type === "pinned" || r.memory.memory_type === "preference").length;
      const memOther = newResults.length - memDynamic - memStatic;

      let memCountMsg = "";
      if (memDynamic > 0) memCountMsg += `Dynamic(${memDynamic}) `;
      if (memStatic > 0) memCountMsg += `Static(${memStatic}) `;
      if (memOther > 0) memCountMsg += `Other(${memOther}) `;

      const categories = categorize(newResults);
      const catSummary = Array.from(categories.entries())
        .map(([label, items]) => `${label}(${items.length})`)
        .join(" · ");

      let toastTitle: string;
      let toastMessage: string;
      
      if (clustered) {
        const clusterCount = clustered.cluster_summaries.length;
        const standaloneCount = clustered.standalone_memories.length;
        toastTitle = `🧠 Context Injected · ${clusterCount} 主题簇${standaloneCount > 0 ? ` · ${standaloneCount} 补充` : ""}`;
        toastMessage = profileInjected 
          ? `Profile: ${profileCountText} · 聚合记忆展示`
          : `聚合记忆展示`;
      } else {
        toastTitle = `🧠 Context Injected · ${newResults.length} fragments`;
        toastMessage = profileInjected 
          ? `Profile: ${profileCountText} · Memories: ${memCountMsg.trim()}${catSummary ? ` · ${catSummary}` : ""}`
          : `${memCountMsg.trim()}${catSummary ? ` · ${catSummary}` : ""}`;
      }

      showToast(tui, toastTitle, toastMessage, "success", toastDelayMs);

      if (!recordResult) {
        showToast(tui, "🔴 Recall Record Failed", `Memories injected but save failed · check API connection`, "warning", toastDelayMs);
      }

      if (keywordDetectedSessions.has(input.sessionID)) {
        output.system.push(KEYWORD_NUDGE);
        keywordDetectedSessions.delete(input.sessionID);
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      if (errMsg.includes("[cerebro]")) {
        // Server returned error (500, etc.) with details
        const cleanMsg = errMsg.replace(/^\[cerebro\]\s*/, "");
        if (cleanMsg.startsWith("500")) {
          showToast(tui, "🧠 Cerebro Server Error", cleanMsg.substring(0, 200), "error");
        } else if (cleanMsg.includes("timed out")) {
          showToast(tui, "🧠 Cerebro Service Timeout", cleanMsg.substring(0, 100), "error");
        } else {
          showToast(tui, "🧠 Cerebro Error", cleanMsg.substring(0, 150), "error");
        }
      } else if (errMsg.includes("fetch") || errMsg.includes("network")) {
        showToast(tui, "🧠 Cerebro Service Unavailable", "Network error · check API connection", "error");
      } else {
        showToast(tui, "🧠 Memory Recall Error", errMsg.substring(0, 100), "error");
      }
    }
  };
}

export function keywordDetectionHook(_client: CerebroClient, _containerTags: string[], threshold: number, _tui: any, _ingestMode: "smart" | "raw" = "smart", config: Partial<OmemPluginConfig> = {}, agentId?: string) {
  const effectiveAgentId = agentId || process.env.OMEM_AGENT_ID || "opencode";
  return async (
    input: { sessionID: string; messageID?: string },
    output: { message: UserMessage; parts: Part[] },
  ) => {
    const textContent = output.parts
      .filter((p): p is any => p.type === "text")
      .map((p) => (p as any).text || (p as any).content || "")
      .join(" ")
      || (output.message as any).content
      || "";

    if (!firstMessages.has(input.sessionID)) {
      firstMessages.set(input.sessionID, textContent);
    }

    if (detectKeyword(textContent)) {
      keywordDetectedSessions.add(input.sessionID);
      logDebug("keywordDetectionHook triggered", { sessionId: input.sessionID });
    }

    const policy = resolveAgentPolicy(effectiveAgentId, config);
    if (policy === "none") {
      return;
    }

    if (!sessionMessages.has(input.sessionID)) {
      sessionMessages.set(input.sessionID, []);
    }
    sessionMessages.get(input.sessionID)!.push({
      role: "user",
      content: textContent,
    });

    const messages = sessionMessages.get(input.sessionID)!;
    if (messages.length >= threshold) {
      // Threshold reached — messages will be processed on next session.idle
    }
  };
}

export function compactingHook(client: CerebroClient, containerTags: string[], tui: any, ingestMode: "smart" | "raw" = "smart", isAutoStoreEnabled?: (sessionId: string | undefined) => boolean, getMainSessionId?: () => string | undefined, sdkClient?: any, config: Partial<OmemPluginConfig> = {}, agentId?: string) {
  const effectiveAgentId = agentId || process.env.OMEM_AGENT_ID || "opencode";
  return async (
    input: { sessionID?: string },
    output: { context: string[]; prompt?: string },
  ) => {
    // Search (read) always runs — even readonly agents need context during compacting
    try {
      const results = await client.searchMemories("*", 20, undefined, containerTags);
      const block = buildContextBlock(results);
      if (block) {
        output.context.push(block);
      }
    } catch {
    }

    // Main session gate: sub-agents must not write memories via compacting
    if (getMainSessionId) {
      const mainId = getMainSessionId();
      if (mainId && input.sessionID && input.sessionID !== mainId) {
        logInfo("compactingHook: non-main session skipped", { sessionID: input.sessionID, mainSessionId: mainId });
        return;
      }
    }

    // Policy gate: only readwrite agents can write memories
    const policy = resolveAgentPolicy(effectiveAgentId, config);
    if (policy !== "readwrite") {
      logInfo("compactingHook blocked by policy", { agentId: effectiveAgentId, policy });
      if (input.sessionID) sessionMessages.delete(input.sessionID);
      return;
    }

    if (input.sessionID && sessionMessages.has(input.sessionID)) {
      if (isAutoStoreEnabled && !isAutoStoreEnabled(input.sessionID)) {
        sessionMessages.delete(input.sessionID);
      } else {
        const messages = sessionMessages.get(input.sessionID)!;
        if (messages.length > 0) {

          // Use main session ID for sub-agent sessions so memories merge into the main session
          const effectiveSessionId = (getMainSessionId?.() || input.sessionID);

          // Detect project name from session info
          let projectName: string | undefined;
          try {
            if (sdkClient && input.sessionID) {
              const sessionInfo = await sdkClient.session.get({ path: { id: input.sessionID } });
              logDebug("compactingHook project.rootPath", { rootPath: sessionInfo?.data?.directory });
              projectName = sessionInfo?.data?.directory
                ? await detectProjectName(sessionInfo.data.directory)
                : undefined;
            }
          } catch (e) {
            logErr("compactingHook detectProjectName failed", { error: String(e) });
          }

          try {
            logInfo("compactingHook ingestMessages called", { msgCount: messages.length, sessionId: effectiveSessionId, agentId: effectiveAgentId });
            const result = await client.ingestMessages(messages, {
              mode: ingestMode,
              tags: [...containerTags, "auto-capture"],
              sessionId: effectiveSessionId,
              projectName: projectName,
              agentId: effectiveAgentId,
            });
            logInfo("compactingHook ingestMessages result", { result: result === null ? "null(blocked)" : "ok" });
            if (result === null) {
              showToast(tui, "🔴 Archive Failed", "Session archive blocked · check spiritual realm status", "error");
            } else {
              showToast(tui, "📦 Session Archived", `${messages.length} residual dialogues archived · merged into the realm`, "success");
            }
          } catch (e) {
            logErr("compactingHook ingestMessages failed", { error: String(e) });
            showToast(tui, "🔴 Archive Failed", "Session archive blocked · spiritual pulse anomaly", "error");
          }
          sessionMessages.delete(input.sessionID);
        }
      }
    }
  };
}

const processedMessageIds = new Set<string>();
const pluginStartTime = Date.now();

export function sessionIdleHook(
  cerebroClient: CerebroClient,
  _containerTags: string[],
  tui: any,
  sdkClient: any,
  _ingestMode: "smart" | "raw" = "smart",
  threshold: number = 0,
  getMainSessionId?: () => string | undefined,
  isAutoStoreEnabled?: (sessionId: string | undefined) => boolean,
  agentId?: string,
  config: Partial<OmemPluginConfig> = {},
  onAgentResolved?: (name: string) => void,
) {
  let idleTimeout: ReturnType<typeof setTimeout> | null = null;
  let isCapturing = false;

  return async (input: { event: { type: string; properties?: any } }) => {
    if (input.event.type !== "session.idle") return;

    logDebug("sessionIdleHook event.properties dump", { keys: Object.keys(input.event.properties || {}), raw: JSON.stringify(input.event.properties).substring(0, 2000) });

    const sessionID = input.event.properties?.sessionID;
    if (!sessionID) return;

    if (isAutoStoreEnabled && !isAutoStoreEnabled(sessionID)) return;

    if (getMainSessionId) {
      const mainId = getMainSessionId();
      if (mainId && sessionID !== mainId) {
        logInfo("sessionIdleHook: non-main session skipped", { sessionID, mainSessionId: mainId });
        return;
      }
    }

    if (idleTimeout) clearTimeout(idleTimeout);

    idleTimeout = setTimeout(async () => {
      if (isCapturing) return;
      isCapturing = true;

      try {
        const response = await sdkClient.session.messages({ path: { id: sessionID } });
        if (!response?.data) return;

        const messages = response.data;
        const conversationMessages: Array<{ role: string; content: string }> = [];
        const newMessageIds: string[] = [];
        let hasNewMessages = false;

        for (const msg of messages) {
          const msgId = msg.info?.id;
          if (!msgId || processedMessageIds.has(msgId)) continue;

          const msgTime = msg.info?.createdAt ? new Date(msg.info.createdAt).getTime() : 0;
          if (msgTime > 0 && msgTime < pluginStartTime) continue;

          const role = msg.info?.role;
          if (role !== "user" && role !== "assistant") continue;

          const textParts = (msg.parts || [])
            .filter((p: any) => p.type === "text" && p.text)
            .map((p: any) => p.text);
          const text = textParts.join("\n").trim();
          if (!text) continue;

          hasNewMessages = true;
          newMessageIds.push(msgId);
          conversationMessages.push({ role, content: text });
        }

        if (!hasNewMessages || conversationMessages.length === 0) return;

        if (threshold > 1 && conversationMessages.length < threshold) {
          return;
        }

        let sessionTitle: string | undefined;
        let projectName: string | undefined;
        let effectiveAgentId = agentId || "opencode";
        try {
          const sessionInfo = await sdkClient.session.get({ path: { id: sessionID } });
          if ((sessionInfo?.data as any)?.agent) {
            effectiveAgentId = (sessionInfo.data as any).agent;
            onAgentResolved?.(effectiveAgentId);
          }
          sessionTitle = sessionInfo?.data?.title;
          projectName = sessionInfo?.data?.directory
            ? await detectProjectName(sessionInfo.data.directory)
            : undefined;
        } catch (e) {
          logErr("sessionIdleHook detectProjectName failed", { error: String(e) });
        }

        logDebug("sessionIdleHook resolved agentId", { effectiveAgentId, fallbackAgentId: agentId });

        const policy = resolveAgentPolicy(effectiveAgentId, config);
        if (policy !== "readwrite") {
          logInfo("sessionIdleHook blocked by policy", { agentId: effectiveAgentId, policy, defaultPolicy: String(config.defaultPolicy ?? "undefined") });
          return;
        }

        try {
          logInfo("sessionIdleHook sessionIngest called", { msgCount: conversationMessages.length, sessionId: sessionID, agentId: effectiveAgentId, title: String(sessionTitle) });
          await cerebroClient.sessionIngest(conversationMessages, sessionID, effectiveAgentId, sessionTitle, projectName);
          logInfo("sessionIdleHook sessionIngest ok");
          for (const id of newMessageIds) {
            processedMessageIds.add(id);
          }
          showToast(tui, "🧠 Memory Sealed", `${conversationMessages.length} dialogues captured · entrusted to the heavens for refinement`, "success");
        } catch (err) {
          logErr("sessionIdleHook sessionIngest failed", { error: String(err) });
          showToast(tui, "🔴 Session Capture Failed", String(err).substring(0, 100), "error");
        }
      } catch (err) {
        const errMsg = err instanceof Error ? err.message : String(err);
        showToast(tui, "🔴 Idle Capture Error", errMsg.substring(0, 100), "error");
      } finally {
        isCapturing = false;
        idleTimeout = null;
      }
    }, 10000);
  };
}
