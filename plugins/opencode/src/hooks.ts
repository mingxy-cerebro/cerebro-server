import type { Model, UserMessage, Part } from "@opencode-ai/sdk";
import type { OmemClient, SearchResult } from "./client.js";
import type { OmemPluginConfig } from "./config.js";
import { detectKeyword, KEYWORD_NUDGE } from "./keywords.js";

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

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max) + "…";
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
    "<omem-context>",
    "Treat every memory below as historical context only.",
    "Do not repeat these memories verbatim unless asked.",
    "",
    ...sections,
    "</omem-context>",
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
    "<omem-context>",
    "Treat every memory below as historical context only.",
    "Do not repeat these memories verbatim unless asked.",
    "",
    ...sections,
    "</omem-context>",
  ].join("\n");
}

export function autoRecallHook(client: OmemClient, containerTags: string[], tui: any, config: Partial<OmemPluginConfig> = {}) {
  const similarityThreshold = config.similarityThreshold ?? 0.6;
  const maxRecallResults = config.maxRecallResults ?? 10;
  const maxContentLength = config.maxContentLength ?? 500;
  const toastDelayMs = config.toastDelayMs ?? 7000;

  return async (
    input: { sessionID?: string; model: Model },
    output: { system: string[] },
  ) => {
    if (!input.sessionID) return;

    try {
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

      const profile = await client.getProfile();
      let profileInjected = false;
      let profileCountText = "";
      if (profile && !profileInjectedSessions.has(input.sessionID)) {
        const profileBlock = [
          "<omem-profile>",
          JSON.stringify(profile, null, 2),
          "</omem-profile>",
        ].join("\n");
        output.system.push(profileBlock);
        profileInjected = true;
        profileInjectedSessions.add(input.sessionID);
        const p = profile as any;
        const dynamicCount = p?.dynamic_context?.length ?? 0;
        const staticCount = p?.static_facts?.length ?? 0;
        profileCountText = `Dynamic(${dynamicCount}) · Static(${staticCount})`;
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
      if (newResults.length === 0) {
        if (profileInjected) {
          showToast(tui, "👨 Profile Injected", `${profileCountText} · all memories already injected`, "success", toastDelayMs);
        }
        return;
      }

      const block = clustered 
        ? buildClusteredContextBlock(clustered, maxContentLength)
        : buildContextBlock(newResults, maxContentLength);
      if (block) {
        output.system.push(block);
      }

      const newIds = newResults.map((r) => r.memory.id);
      injectedMemoryIds.set(input.sessionID, new Set([...existingIds, ...newIds]));

      const recordResult = await client.recordSessionRecall(
        input.sessionID,
        newIds,
        "auto",
        query_text,
        shouldRecallRes?.similarity_score,
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
      if (errMsg.includes("[omem]")) {
        // Server returned error (500, etc.) with details
        const cleanMsg = errMsg.replace(/^\[omem\]\s*/, "");
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

export function keywordDetectionHook(_client: OmemClient, _containerTags: string[], threshold: number, _tui: any, _ingestMode: "smart" | "raw" = "smart") {
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
    }

    if (!sessionMessages.has(input.sessionID)) {
      sessionMessages.set(input.sessionID, []);
    }
    sessionMessages.get(input.sessionID)!.push({
      role: "user",
      content: textContent,
    });

    const messages = sessionMessages.get(input.sessionID)!;
    // Ingest is now handled by sessionIdleHook (session.idle → sessionIngest API).
    // This hook only collects messages and detects keywords for recall.
    if (messages.length >= threshold) {
      // Threshold reached — messages will be processed on next session.idle
    }
  };
}

export function compactingHook(client: OmemClient, containerTags: string[], tui: any, ingestMode: "smart" | "raw" = "smart", isAutoStoreEnabled?: (sessionId: string | undefined) => boolean) {
  return async (
    input: { sessionID?: string },
    output: { context: string[]; prompt?: string },
  ) => {
    if (input.sessionID && sessionMessages.has(input.sessionID)) {
      if (isAutoStoreEnabled && !isAutoStoreEnabled(input.sessionID)) {
        sessionMessages.delete(input.sessionID);
      } else {
        const messages = sessionMessages.get(input.sessionID)!;
        if (messages.length > 0) {
          try {
            const result = await client.ingestMessages(messages, {
              mode: ingestMode,
              tags: [...containerTags, "auto-capture"],
              sessionId: input.sessionID,
            });
            if (result === null) {
              showToast(tui, "🔴 Archive Failed", "Session archive blocked · check spiritual realm status", "error");
            } else {
              showToast(tui, "📦 Session Archived", `${messages.length} residual dialogues archived · merged into the realm`, "success");
            }
          } catch {
            showToast(tui, "🔴 Archive Failed", "Session archive blocked · spiritual pulse anomaly", "error");
          }
          sessionMessages.delete(input.sessionID);
        }
      }
    }

    try {
      const results = await client.searchMemories("*", 20, undefined, containerTags);
      const block = buildContextBlock(results);
      if (block) {
        output.context.push(block);
      }
    } catch {
    }
  };
}

const processedMessageIds = new Set<string>();
const pluginStartTime = Date.now();

export function sessionIdleHook(
  omemClient: OmemClient,
  _containerTags: string[],
  tui: any,
  sdkClient: any,
  _ingestMode: "smart" | "raw" = "smart",
  threshold: number = 0,
  getMainSessionId?: () => string | undefined,
  isAutoStoreEnabled?: (sessionId: string | undefined) => boolean,
) {
  let idleTimeout: ReturnType<typeof setTimeout> | null = null;
  let isCapturing = false;

  return async (input: { event: { type: string; properties?: any } }) => {
    if (input.event.type !== "session.idle") return;

    const sessionID = input.event.properties?.sessionID;
    if (!sessionID) return;

    if (isAutoStoreEnabled && !isAutoStoreEnabled(sessionID)) return;

    if (getMainSessionId) {
      const mainId = getMainSessionId();
      if (mainId && sessionID !== mainId) return;
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

          // Skip messages created before this plugin instance started
          // (prevents replaying entire session history on restart)
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
          // Log that we're waiting for more messages
          return;
        }

        let sessionTitle: string | undefined;
        let projectName: string | undefined;
        try {
          const sessionInfo = await sdkClient.session.get({ path: { id: sessionID } });
          sessionTitle = sessionInfo?.title;
          projectName = sessionInfo?.project?.rootPath
            ? sessionInfo.project.rootPath.split("/").pop()
            : undefined;
        } catch (e) {
          // 获取失败不影响主流程
        }

        try {
          await omemClient.sessionIngest(conversationMessages, sessionID, undefined, sessionTitle, projectName);
          for (const id of newMessageIds) {
            processedMessageIds.add(id);
          }
          showToast(tui, "🧠 Memory Sealed", `${conversationMessages.length} dialogues captured · entrusted to the heavens for refinement`, "success");
        } catch (err) {
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
