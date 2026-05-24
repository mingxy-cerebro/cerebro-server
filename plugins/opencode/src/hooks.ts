import type { Model, UserMessage, Part } from "@opencode-ai/sdk";
import type { CerebroClient, SearchResult } from "./client.js";
import { type OmemPluginConfig, resolveAgentPolicy } from "./config.js";
import { detectSaveKeyword, detectRecallKeyword as _detectRecallKeyword, KEYWORD_NUDGE, RECALL_NUDGE as _RECALL_NUDGE } from "./keywords.js";
import { logDebug, logInfo, logError as logErr } from "./logger.js";
import { readFile } from "node:fs/promises";
import { stripPrivateContent } from "./privacy.js";

const BOUNDARY_SEARCH_RATIO = 0.6;
const MIN_ITEM_CONTENT_CHARS = 100;
const MIN_CONTENT_CHARS = 1000;
const MIN_CONTENT_LENGTH = 50;

const projectNameCache = new Map<string, string>();

function appendToSystem(system: string[], content: string) {
  if (system.length > 0) {
    system[system.length - 1] += "\n\n" + content;
  } else {
    system.push(content);
  }
}

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

export function showToast(tui: any, title: string, message: string, variant: string = "info", delayMs: number = 7000) {
  if (!tui) return;
  setTimeout(() => {
    try {
      tui.showToast({ body: { title, message, variant, duration: 5000 } });
    } catch (err) {
      logErr("showToast failed", { error: String(err) });
    }
  }, delayMs);
}

const SYSTEM_INJECTION_PATTERNS: RegExp[] = [
  /<!--\s*OMO_INTERNAL_INITIATOR\s*-->/,
  /^\[SYSTEM DIRECTIVE:/,
  /^\[restore checkpointed/,
  /^\[session recovered/,
  /^<system-reminder>/,
  /^<EXTREMELY_IMPORTANT>/,
  /^\[CONTEXT\]/,
  /^\[GOAL\]/,
  /^## 任务[：:]/,
  /^## 改动/,
  /^Analyze the attached file/,
  /^Provide ONLY the extracted/,
  /^Called the Read tool/,
  /^MANDATORY delegate_task/,
  /^[▣▪]\s*DCP/,
];

const MODE_TAG_PATTERN = /^\[(?:search-mode|analyze-mode)\][\s\S]*?\n---\n?/;
const MODE_TAG_LINE = /^\[(?:search-mode|analyze-mode)\]\s*\n/;

function extractUserRequest(content: string): string {
  const match = content.match(/<user-request>([\s\S]*?)<\/user-request>/);
  let text = match ? match[1].trim() : content;

  // [search-mode] / [analyze-mode]: 剥离标签+系统指令+分隔线，保留用户实际内容
  const stripped = text.replace(MODE_TAG_PATTERN, "");
  if (stripped !== text && stripped.trim()) {
    text = stripped.trim();
  } else {
    text = text.replace(MODE_TAG_LINE, "").trim();
  }

  for (const pattern of SYSTEM_INJECTION_PATTERNS) {
    if (pattern.test(text)) return "";
  }

  return text;
}

const saveKeywordDetectedSessions = new Set<string>();
const firstMessages = new Map<string, string>();
const sessionMessages = new Map<string, Array<{ role: string; content: string }>>();
export const profileInjectedSessions = new Map<string, number>();
const lastProfileBlock = new Map<string, { content: string; count: number }>();
const lastUserMsgCount = new Map<string, number>();
const summarizedSessions = new Set<string>();

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

function formatMemoryLine(r: SearchResult, maxContentLength: number): string {
  const age = formatRelativeAge(r.memory.created_at);
  const tags = r.memory.tags.length > 0 ? ` [${r.memory.tags.join(", ")}]` : "";
  const idTag = ` [id:${r.memory.id}]`;
  const relTag = r.memory.relations && r.memory.relations.length > 0
    ? ` [rel:${r.memory.relations.map((rel) => rel.target_id).join(",")}]`
    : "";
  const refineTag = r.refine_relevance?.trim() ? ` [${r.refine_relevance.trim()}]` : "";
  const content = truncate(r.memory.content, maxContentLength);
  return `  - (${age}${idTag}${relTag}${refineTag}${tags}) ${content}`;
}

const FETCH_POLICY = [
  "<cerebro-fetch-policy>",
  "IMPORTANT: Each memory above is a condensed summary. The full version contains critical details that may change your response quality.",
  "You MUST use memory_get(\"id\") to retrieve the complete content, or memory_search(\"query\") to find specific memories before making decisions based on any summary.",
  "Do NOT rely on condensed summaries alone — depth of recall determines quality of response.",
  "</cerebro-fetch-policy>",
].join("\n");

const INJECTION_MAX_CHARS_FALLBACK = 4000;

interface InjectionResult {
  text: string;
  profileCount: number;
  memoryCount: number;
  projectMemoryCount: number;
}

export async function buildMemoryInjection(
  client: CerebroClient,
  projectPath: string | undefined,
  query: string,
  config: Partial<OmemPluginConfig>,
): Promise<InjectionResult> {
  const maxChars = config.content?.maxContentLength ?? INJECTION_MAX_CHARS_FALLBACK;

  const [profile, projectMemories, searchResults] = await Promise.all([
    Promise.race([
      client.getInjection(),
      new Promise<null>((resolve) => setTimeout(() => resolve(null), 3000)),
    ]).catch(() => null),
    Promise.race([
      client.listRecent(5, projectPath),
      new Promise<never[]>((resolve) => setTimeout(() => resolve([]), 2000)),
    ]).catch(() => []),
    query
      ? Promise.race([
          client.searchMemories(query, 10, undefined, undefined, projectPath),
          new Promise<never[]>((resolve) => setTimeout(() => resolve([]), 3000)),
        ]).catch(() => [])
      : Promise.resolve([]),
  ]);

  const sections: string[] = ["[CEREBRO-MEMORY]", ""];

  if (profile?.content) {
    sections.push(profile.content);
    sections.push("");
  }

  const seenIds = new Set<string>();

  if (projectMemories.length > 0) {
    sections.push("## Recent Project Activity");
    for (const m of projectMemories) {
      seenIds.add(m.id);
      const age = formatRelativeAge(m.updated_at || m.created_at) || "unknown";
      const content = truncate(m.content, 200);
      sections.push(`- (${age}) ${content}`);
    }
    sections.push("");
  }

  const dedupedResults = (searchResults || []).filter((r) => !seenIds.has(r.memory.id));
  if (dedupedResults.length > 0) {
    sections.push("## Relevant Memories");
    for (const r of dedupedResults) {
      const age = formatRelativeAge(r.memory.created_at) || "unknown";
      const content = truncate(r.memory.content, 300);
      sections.push(`- (${age}) ${content}`);
    }
    sections.push("");
  }

  sections.push("[/CEREBRO-MEMORY]");

  let text = sections.join("\n");
  if (text.length > maxChars) {
    const cutoff = text.lastIndexOf('\n', maxChars);
    text = text.slice(0, cutoff > 0 ? cutoff : maxChars) + "\n…\n[/CEREBRO-MEMORY]";
  }

  return {
    text,
    profileCount: profile?.preference_count ?? 0,
    memoryCount: dedupedResults?.length ?? 0,
    projectMemoryCount: projectMemories.length,
  };
}

const injectedSessions = new Set<string>();

export function chatMessageRecallHook(
  client: CerebroClient,
  _containerTags: string[],
  tui: any,
  config: Partial<OmemPluginConfig> = {},
  getAgentName?: () => string,
  directory?: string,
) {
  return async (
    input: { sessionID: string; messageID?: string },
    output: { message: UserMessage; parts: Part[] },
  ) => {
    if (!input.sessionID) return;
    if (injectedSessions.has(input.sessionID)) return;

    const agentId = getAgentName?.() || process.env.OMEM_AGENT_ID || "opencode";
    const policy = resolveAgentPolicy(agentId, config);
    if (policy === "none") {
      injectedSessions.add(input.sessionID);
      return;
    }

    const textContent = output.parts
      .filter((p: any) => p.type === "text")
      .map((p: any) => p.text || (p as any).content || "")
      .join(" ")
      || (output.message as any).content
      || "";

    const query = extractUserRequest(textContent);

    const TRIVIAL_PATTERNS = /^(hi|hello|hey|你好|嗨|嗯|ok|okay|好的|收到|\s*)$/i;
    if (!query || TRIVIAL_PATTERNS.test(query.trim())) {
      logDebug("chatMessageRecallHook: trivial query, will retry next turn", { sessionId: input.sessionID });
      return;
    }

    try {
      const injection = await buildMemoryInjection(client, directory, query, config);

      const hasContent = (injection.profileCount ?? 0) > 0
        || (injection.memoryCount ?? 0) > 0
        || (injection.projectMemoryCount ?? 0) > 0;

      if (injection.text && hasContent && injection.text.length > 20) {
        injectedSessions.add(input.sessionID);

        output.parts.unshift({
          type: "text",
          text: injection.text,
          synthetic: true,
        } as any);

        showToast(tui, "🧠 Memory Injected",
          `${injection.profileCount} prefs · ${injection.projectMemoryCount} project · ${injection.memoryCount} relevant`,
          "success");
      } else if (!hasContent) {
        logDebug("chatMessageRecallHook: no content available, will retry next turn", {
          sessionId: input.sessionID,
          profileCount: injection.profileCount,
          memoryCount: injection.memoryCount,
          projectMemoryCount: injection.projectMemoryCount,
        });
        showToast(tui, "🧠 Memory Unavailable", "API timeout or no memories yet", "warning");
      }
    } catch (err) {
      logErr("chatMessageRecallHook failed", { error: String(err) });
      showToast(tui, "🧠 Memory Injection Failed", "Check connection", "error");
    }
  };
}

/**
 * Score-weighted budget allocation: high-score memories get more chars.
 * Falls back to uniform distribution when totalScore === 0 or all scores equal.
 */
interface ContextBlockResult {
  text: string;
  injectedMemoryIds: string[];
  injectedCount: number;
}

function buildContextBlock(
  results: SearchResult[],
  budget: number,
  maxContentLength: number = 500,
  minItemChars: number = MIN_ITEM_CONTENT_CHARS,
): ContextBlockResult {
  const empty: ContextBlockResult = { text: "", injectedMemoryIds: [], injectedCount: 0 };
  if (results.length === 0) return empty;

  const totalScore = results.reduce((sum, r) => sum + r.score, 0);

  const grouped = categorize(results);
  const sections: string[] = [];

  for (const [label, items] of grouped) {
    const lines = items.map((r) => {
      const itemMaxLen = totalScore > 0
        ? Math.min(maxContentLength, Math.max(minItemChars, Math.floor((r.score / totalScore) * budget)))
        : Math.min(maxContentLength, Math.max(minItemChars, Math.floor(budget / results.length)));
      return formatMemoryLine(r, itemMaxLen);
    });
    sections.push(`[${label}]\n${lines.join("\n")}`);
  }

  return {
    text: [
      "<cerebro-context>",
      "",
      ...sections,
      "</cerebro-context>",
    ].join("\n"),
    injectedMemoryIds: results.map((r) => r.memory.id),
    injectedCount: results.length,
  };
}

export function autoRecallHook(client: CerebroClient, containerTags: string[], tui: any, config: Partial<OmemPluginConfig> = {}, getAgentName?: () => string, directory?: string) {
  const similarityThreshold = config.recall?.similarityThreshold ?? 0.4;
  const maxRecallResults = config.recall?.maxRecallResults ?? 10;
  const fetchMultiplier = config.recall?.fetchMultiplier ?? 3;
  const topkCapMultiplier = config.recall?.topkCapMultiplier ?? 2;
  const mmrJaccardThreshold = config.recall?.mmrJaccardThreshold ?? 0.85;
  const mmrPenaltyFactor = config.recall?.mmrPenaltyFactor ?? 0.5;
  const phase2Multiplier = config.recall?.phase2Multiplier ?? 2;
  const llmMaxEval = config.recall?.llmMaxEval ?? 15;
  const refineStrategy = config.recall?.refineStrategy ?? "balanced";
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
      logDebug("autoRecallHook start", { sessionId: input.sessionID, agentId, policy, similarityThreshold, maxRecallResults, fetchMultiplier, topkCapMultiplier, mmrJaccardThreshold, mmrPenaltyFactor, phase2Multiplier, llmMaxEval, refineStrategy });
      const messages = sessionMessages.get(input.sessionID) ?? [];
      const userMessages = messages.filter((m) => m.role === "user");

      const prevCount = lastUserMsgCount.get(input.sessionID) ?? 0;
      if (userMessages.length <= prevCount) {
        logDebug("autoRecallHook skipped: no new user message", { sessionId: input.sessionID, prevCount, currentCount: userMessages.length });
        return;
      }
      lastUserMsgCount.set(input.sessionID, userMessages.length);

      // --- Profile Fetch (V2 inject API with TTL gate + module-level cache) ---
      const profileTtlMs = config.profile?.ttlMs ?? 300000; // default 5 minutes
      const lastInjected = profileInjectedSessions.get(input.sessionID);
      const profileTtlExpired = !lastInjected || (Date.now() - lastInjected > profileTtlMs);

      let profileBlock = "";
      let profileCountText = "";

      if (profileTtlExpired) {
        const maxRetries = 2;
        for (let attempt = 0; attempt <= maxRetries; attempt++) {
          try {
            const injection = await client.getInjection(directory || process.env.OMEM_PROJECT_DIR);
            if (injection?.content) {
              profileBlock = injection.content;
              profileCountText = `${injection.preference_count} preferences`;
              profileInjectedSessions.set(input.sessionID, Date.now());
              lastProfileBlock.set(input.sessionID, { content: profileBlock, count: injection.preference_count });
              logDebug("autoRecallHook profile fetched (V2 injection)", { preferenceCount: injection.preference_count, estimatedTokens: injection.estimated_tokens });
            }
            break;
          } catch (e) {
            if (attempt < maxRetries) {
              logDebug("autoRecallHook getInjection retry", { attempt: attempt + 1, error: String(e) });
            } else {
              logErr("autoRecallHook getInjection failed after retries", { error: String(e) });
              showToast(tui, "⚠️ Profile Inject Failed", "Preference injection skipped · will retry next turn", "error", toastDelayMs);
            }
          }
        }
      } else {
        // TTL 未过期 — 从缓存恢复 profile 内容
        const cached = lastProfileBlock.get(input.sessionID);
        if (cached) {
          profileBlock = cached.content;
          profileCountText = `${cached.count} preferences`;
          logDebug("autoRecallHook profile restored from cache", { preferenceCount: cached.count, contentLen: cached.content.length });
        }
      }

      // After compacting, sessionMessages is cleared but firstMessages gets repopulated
      // by keywordDetectionHook with compact summary — skip recall in this transient state
      if (userMessages.length === 0) {
        logDebug("autoRecallHook skipped: no user messages in session (post-compacting?)", { sessionId: input.sessionID });
        return;
      }

      const rawQuery = userMessages[userMessages.length - 1]?.content || firstMessages.get(input.sessionID) || "";
      const query_text = extractUserRequest(rawQuery);
      if (!query_text) {
        logDebug("autoRecallHook filtered system injection (profile already injected above)", { rawQueryPrefix: rawQuery.slice(0, 60) });
        return;
      }
      const last_query_text = userMessages.length >= 2 ? userMessages[userMessages.length - 2].content : undefined;

      const projectTags = containerTags.filter(t => t.startsWith("omem_project_"));

      const conversationContext = userMessages.length >= 2
        ? userMessages.slice(-4, -1).map((m) => {
          const stripped = stripPrivateContent(m.content);
          return stripped.length > 200 ? stripped.slice(0, 200) : stripped;
        })
        : undefined;

      const shouldRecallRes = await client.shouldRecall(
        query_text, last_query_text, input.sessionID,
        similarityThreshold, maxRecallResults,
        projectTags.length > 0 ? projectTags : undefined,
        conversationContext && conversationContext.length > 0 ? conversationContext : undefined,
        {
          fetch_multiplier: fetchMultiplier,
          topk_cap_multiplier: topkCapMultiplier,
          mmr_jaccard_threshold: mmrJaccardThreshold,
          mmr_penalty_factor: mmrPenaltyFactor,
          phase2_multiplier: phase2Multiplier,
          llm_max_eval: llmMaxEval,
          refine_strategy: refineStrategy,
        },
        directory || process.env.OMEM_PROJECT_DIR,
      );

      if (!shouldRecallRes) {
        showToast(tui, "🧠 Cerebro Service Unavailable", "Unable to reach memory API · check connection", "error", toastDelayMs);
        return;
      }
      logDebug("autoRecallHook shouldRecall result", { shouldRecall: shouldRecallRes.should_recall, confidence: shouldRecallRes.confidence, memCount: shouldRecallRes.memories?.length ?? 0, discardedCount: shouldRecallRes.discarded?.length ?? 0 });

      const storedMemoryIds = shouldRecallRes.memories?.map((r) => r.memory.id) ?? [];
      const storedDiscardedIds = shouldRecallRes.discarded?.map((d) => d.memory_id) ?? [];
      const maxScore = storedMemoryIds.length > 0
        ? Math.max(...(shouldRecallRes.memories?.map((r) => r.score) ?? [0]))
        : 0;

      const createEventAndReturn = async (
        opts: {
          injectedContent?: string;
          actualProfileInjected: boolean;
          actualProfileContent?: string;
          actualInjectedCount: number;
          injectedMemoryIds: string[];
          keptCount: number;
          discardedCount: number;
        },
      ): Promise<string | undefined> => {
        try {
          const items = [
                ...(shouldRecallRes.memories?.map((r) => ({
                  memory_id: r.memory.id,
                  score: r.score,
                  refine_relevance: r.refine_relevance,
                  refine_reasoning: r.refine_reasoning,
                  is_kept: opts.injectedMemoryIds.includes(r.memory.id),
                })) ?? []),
                ...(shouldRecallRes.discarded?.map((d) => ({
                  memory_id: d.memory_id,
                  score: d.score,
                  refine_relevance: d.refine_relevance,
                  refine_reasoning: d.refine_reasoning,
                  is_kept: false,
                })) ?? []),
              ];
          const result = await client.createRecallEvent({
            session_id: input.sessionID!,
            recall_type: "auto",
            query_text,
            max_score: maxScore,
            llm_confidence: shouldRecallRes.confidence ?? 0,
            profile_injected: opts.actualProfileInjected,
            kept_count: opts.keptCount,
            discarded_count: opts.discardedCount,
            injected_count: opts.actualInjectedCount,
            profile_content: opts.actualProfileContent,
            injected_content: opts.injectedContent,
            items: items.length > 0 ? items : undefined,
          });
          return result?.event_id;
        } catch (e) {
          logErr("autoRecallHook createRecallEvent failed", { error: String(e) });
          return undefined;
        }
      };

      if (!shouldRecallRes.should_recall) {
        if (profileTtlExpired && profileBlock) {
          appendToSystem(output.system, profileBlock);
          logDebug("autoRecallHook profile injected (no-recall path)", { sessionId: input.sessionID, outputSystemLength: output.system.length });

          createEventAndReturn({
            keptCount: 0,
            discardedCount: 0,
            actualProfileInjected: true,
            actualProfileContent: profileBlock,
            actualInjectedCount: 0,
            injectedMemoryIds: [],
          });

          showToast(tui, "👨 Profile Injected", `${profileCountText} · no recall needed`, "success", toastDelayMs);
        }
        return;
      }

      const results = shouldRecallRes.memories ?? [];

      // --- Token Budget Calculation ---
      const profileChars = profileBlock ? profileBlock.length : 0;
      const budgetRemaining = maxContentChars - profileChars;
      if (budgetRemaining < 0) {
        logDebug("autoRecallHook budget overflow", { profileChars, maxContentChars, deficit: -budgetRemaining });
      }
      logDebug("autoRecallHook budget", { 
        maxContentChars, profileChars, budgetRemaining,
        configuredMax: maxContentLength,
      });

      const ctxResult = buildContextBlock(results, budgetRemaining, maxContentLength, MIN_ITEM_CONTENT_CHARS);
      if (ctxResult.text) {
        appendToSystem(output.system, ctxResult.text);
        appendToSystem(output.system, FETCH_POLICY);
        logDebug("autoRecallHook block injected to output.system", {
          sessionId: input.sessionID,
          blockPreview: ctxResult.text.slice(0, 200),
          outputSystemLength: output.system.length,
        });
      } else {
        logDebug("autoRecallHook block was EMPTY — no injection", { sessionId: input.sessionID });
      }

      if (profileTtlExpired && profileBlock) {
        appendToSystem(output.system, profileBlock);
        logDebug("autoRecallHook profile injected after context", { sessionId: input.sessionID, outputSystemLength: output.system.length });
      }

      logDebug("autoRecallHook injection complete", { sessionId: input.sessionID });

      const didInjectProfile = !!profileBlock;
      const didInjectContext = !!ctxResult.text;

      createEventAndReturn({
        keptCount: ctxResult.injectedCount,
        discardedCount: storedDiscardedIds.length,
        injectedContent: didInjectContext ? ctxResult.text : undefined,
        actualProfileInjected: didInjectProfile,
        actualProfileContent: profileBlock || undefined,
        actualInjectedCount: ctxResult.injectedCount,
        injectedMemoryIds: ctxResult.injectedMemoryIds,
      });

      // --- Toast (every branch shows toast) ---
      if (didInjectProfile && didInjectContext) {
        showToast(tui, "🧠 Context + Profile Injected", `${profileCountText} · recall active`, "success", toastDelayMs);
      } else if (didInjectProfile) {
        showToast(tui, "👨 Profile Injected", `${profileCountText} · no recall needed`, "success", toastDelayMs);
      } else if (didInjectContext) {
        showToast(tui, "🧠 Context Injected", `Recall active · profile cached`, "success", toastDelayMs);
      } else {
        showToast(tui, "🧠 Cerebro", "profile cached · no recall needed", "info", toastDelayMs);
      }

      if (saveKeywordDetectedSessions.has(input.sessionID)) {
        appendToSystem(output.system, KEYWORD_NUDGE);
        saveKeywordDetectedSessions.delete(input.sessionID);
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

    if (detectSaveKeyword(textContent)) {
      saveKeywordDetectedSessions.add(input.sessionID);
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

export function createCerebroCompactionPrompt(
  context: string[],
  projectMemories: SearchResult[],
): string {
  const sections: string[] = [
    "[Cerebro Compaction Context]",
    "",
    "## 1. User's Original Request",
    "Preserve the user's verbatim original request from the conversation above.",
    "",
    "## 2. Final Goal",
    "What is the ultimate objective the user wants to achieve?",
    "",
    "## 3. Work Completed",
    "List all completed work with file paths and technical decisions made.",
    "",
    "## 4. Remaining Tasks",
    "What is still unfinished or pending?",
    "",
    "## 5. Prohibited Actions",
    "Key constraints and forbidden operations to remember.",
    "",
    "## 6. Existing Project Knowledge",
  ];

  if (projectMemories.length > 0) {
    const memBlock = projectMemories
      .slice(0, 10)
      .map((r) => {
        const content = r.memory.content ?? "";
        const truncated = content.length > 200 ? content.slice(0, 200) + "..." : content;
        return `  - [${r.memory.category ?? "general"}] ${truncated}`;
      })
      .join("\n");
    sections.push(memBlock);
  } else {
    sections.push("  (No project memories retrieved)");
  }

  if (context.length > 0) {
    sections.push("");
    sections.push("### Additional Context");
    sections.push(...context);
  }

  sections.push("");
  sections.push("IMPORTANT: Output must preserve the user's original language (Chinese/English/etc). Do not translate.");

  return sections.join("\n");
}

export function compactingHook(client: CerebroClient, containerTags: string[], tui: any, ingestMode: "smart" | "raw" = "smart", isAutoStoreEnabled?: (sessionId: string | undefined) => boolean, getMainSessionId?: () => string | undefined, sdkClient?: any, config: Partial<OmemPluginConfig> = {}, agentId?: string, directory?: string) {
  const effectiveAgentId = agentId || process.env.OMEM_AGENT_ID || "opencode";
  return async (
    input: { sessionID?: string },
    output: { context: string[]; prompt?: string },
  ) => {
    logInfo("compactingHook triggered", { sessionId: input.sessionID, hasSessionMessages: sessionMessages.has(input.sessionID || "") });

    // Search (read) always runs — even readonly agents need context during compacting
    try {
      const results = await client.searchMemories("*", 20, undefined, containerTags);
      const compactionPrompt = createCerebroCompactionPrompt(output.context, results);
      if (output.prompt !== undefined) {
        output.prompt = compactionPrompt;
      } else if (output.context.length > 0) {
        output.context[output.context.length - 1] += "\n\n" + compactionPrompt;
      } else {
        output.context.push(compactionPrompt);
      }
      if (output.context.length > 0) {
        output.context[output.context.length - 1] += "\n\n" + FETCH_POLICY;
      } else {
        output.context.push(FETCH_POLICY);
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
      if (input.sessionID) {
        sessionMessages.delete(input.sessionID);
        profileInjectedSessions.delete(input.sessionID);
        lastUserMsgCount.delete(input.sessionID);
        firstMessages.delete(input.sessionID);
      }
      return;
    }

    const effectiveSessionId = (getMainSessionId?.() || input.sessionID);

    // Resolve project name (shared by ingest + poll)
    let projectName: string | undefined;
    let projectPath: string | undefined;
    try {
      if (sdkClient && input.sessionID) {
        const sessionInfo = await sdkClient.session.get({ path: { id: input.sessionID } });
        logDebug("compactingHook project.rootPath", { rootPath: sessionInfo?.data?.directory });
        projectPath = sessionInfo?.data?.directory || directory || process.env.OMEM_PROJECT_DIR;
        projectName = sessionInfo?.data?.directory
          ? await detectProjectName(sessionInfo.data.directory)
          : undefined;
      }
    } catch (e) {
      logErr("compactingHook detectProjectName failed", { error: String(e) });
    }
    if (!projectPath) {
      projectPath = directory || process.env.OMEM_PROJECT_DIR;
    }

    // --- Phase 1: Ingest tracked messages from sessionMessages (if available) ---
    if (input.sessionID && sessionMessages.has(input.sessionID)) {
      if (isAutoStoreEnabled && !isAutoStoreEnabled(input.sessionID)) {
        sessionMessages.delete(input.sessionID);
        profileInjectedSessions.delete(input.sessionID);
        lastUserMsgCount.delete(input.sessionID);
        firstMessages.delete(input.sessionID);
      } else {
        const messages = sessionMessages.get(input.sessionID)!;
        if (messages.length > 0) {
          try {
            logInfo("compactingHook ingestMessages called", { msgCount: messages.length, sessionId: effectiveSessionId, agentId: effectiveAgentId });
            const result = await client.ingestMessages(messages, {
              mode: ingestMode,
              tags: [...containerTags, "auto-capture"],
              sessionId: effectiveSessionId,
              projectName: projectName,
              agentId: effectiveAgentId,
              projectPath,
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
        }
      }
      // Cleanup tracked messages regardless of ingest result
      sessionMessages.delete(input.sessionID);
      profileInjectedSessions.delete(input.sessionID);
      lastUserMsgCount.delete(input.sessionID);
      firstMessages.delete(input.sessionID);
      processedMessageIds.delete(input.sessionID);
      injectedSessions.delete(input.sessionID);
      if (input.sessionID) {
        logDebug("compactingHook cleared session state", { sessionID: input.sessionID });
      }
    }

    // After compacting, clear profile TTL so next autoRecallHook re-injects profile
    if (input.sessionID) {
      profileInjectedSessions.delete(input.sessionID);
      lastUserMsgCount.delete(input.sessionID);
      processedMessageIds.delete(input.sessionID);
      injectedSessions.delete(input.sessionID);
      logDebug("compactingHook cleared profile TTL for re-injection", { sessionID: input.sessionID });
    }
  };
}

export function autocontinueHook(
  client: CerebroClient,
  containerTags: string[],
  tui: any,
  ingestMode: "smart" | "raw" = "smart",
  isAutoStoreEnabled?: (sessionId: string | undefined) => boolean,
  getMainSessionId?: () => string | undefined,
  sdkClient?: any,
  config: Partial<OmemPluginConfig> = {},
  agentId?: string,
  directory?: string,
) {
  const effectiveAgentId = agentId || process.env.OMEM_AGENT_ID || "opencode";
  return async (
    input: {
      sessionID: string;
      agent: string;
      model: Model;
      message: UserMessage;
      overflow: boolean;
    },
    _output: { enabled: boolean },
  ) => {
    try {
      const policy = resolveAgentPolicy(effectiveAgentId, config);
      if (policy !== "readwrite") {
        logInfo("autocontinueHook blocked by policy", { agentId: effectiveAgentId, policy });
        return;
      }

      if (isAutoStoreEnabled && !isAutoStoreEnabled(input.sessionID)) {
        logInfo("autocontinueHook skipped: auto-store disabled", { sessionId: input.sessionID });
        return;
      }

      const effectiveSessionId = getMainSessionId?.() || input.sessionID;

      if (!sdkClient) {
        logInfo("autocontinueHook skipped: no sdkClient", { sessionId: input.sessionID });
        return;
      }

      let summaryText: string | undefined;
      try {
        const response = await sdkClient.session.messages({ path: { id: input.sessionID } });
        if (response?.data) {
          const targetMsg = response.data.find(
            (msg: any) => msg.info?.id === input.message.id,
          );
          if (targetMsg?.parts) {
            const textParts = (targetMsg.parts as any[])
              .filter((p: any) => p.type === "text" && p.text)
              .map((p: any) => p.text);
            summaryText = textParts.join("\n").trim();
          }
        }
      } catch (e) {
        logErr("autocontinueHook failed to fetch message parts", { error: String(e) });
      }

      if (!summaryText) {
        logInfo("autocontinueHook skipped: no summary text found", { sessionId: input.sessionID, messageId: input.message.id });
        return;
      }

      let projectName: string | undefined;
      let projectPath: string | undefined;
      try {
        const sessionInfo = await sdkClient.session.get({ path: { id: input.sessionID } });
        projectPath = sessionInfo?.data?.directory || directory || process.env.OMEM_PROJECT_DIR;
        projectName = sessionInfo?.data?.directory
          ? await detectProjectName(sessionInfo.data.directory)
          : undefined;
      } catch (e) {
        logErr("autocontinueHook detectProjectName failed", { error: String(e) });
      }
      if (!projectPath) {
        projectPath = directory || process.env.OMEM_PROJECT_DIR;
      }

      const messages = [{ role: "user" as const, content: summaryText }];
      logInfo("autocontinueHook storing compact summary", {
        summaryLen: summaryText.length,
        sessionId: effectiveSessionId,
        agentId: effectiveAgentId,
        overflow: input.overflow,
        projectName,
      });

      const result = await client.ingestMessages(messages, {
        mode: ingestMode,
        tags: [...containerTags, "auto-capture", "compact-summary"],
        sessionId: effectiveSessionId,
        projectName: projectName,
        agentId: effectiveAgentId,
        projectPath,
      });

      logInfo("autocontinueHook store result", { result: result === null ? "null(blocked)" : "ok" });
      if (result === null) {
        showToast(tui, "🔴 Compact Summary Failed", "Storage blocked · check server status", "error");
      } else {
        showToast(tui, "📦 Compact Summary Stored", "Session summary archived to memory", "success");
      }
    } catch (e) {
      logErr("autocontinueHook failed", { error: String(e) });
    }
  };
}

const processedMessageIds = new Map<string, Set<string>>();
const pluginStartTime = Date.now();

export function sessionIdleHook(
  cerebroClient: CerebroClient,
  containerTags: string[],
  tui: any,
  sdkClient: any,
  ingestMode: "smart" | "raw" = "smart",
  threshold: number = 0,
  getMainSessionId?: () => string | undefined,
  isAutoStoreEnabled?: (sessionId: string | undefined) => boolean,
  agentId?: string,
  config: Partial<OmemPluginConfig> = {},
  onAgentResolved?: (name: string) => void,
  directory?: string,
) {
  let idleTimeout: ReturnType<typeof setTimeout> | null = null;
  let isCapturing = false;

  async function handleSummaryCapture(props: any) {
    const info = props?.info;
    if (!info) return;
    if (info.role !== "assistant" || !info.summary || !info.finish) return;

    const sessionID = info.sessionID;
    if (!sessionID) return;

    if (summarizedSessions.has(sessionID)) return;
    summarizedSessions.add(sessionID);

    if (!sdkClient) {
      logInfo("handleSummaryCapture skipped: no sdkClient", { sessionID });
      return;
    }

    logInfo("handleSummaryCapture triggered", { sessionID });

    if (getMainSessionId) {
      const mainId = getMainSessionId();
      if (mainId && sessionID !== mainId) {
        logInfo("handleSummaryCapture: non-main session skipped", { sessionID, mainSessionId: mainId });
        return;
      }
    }

    const effectiveAgentId = agentId || process.env.OMEM_AGENT_ID || "opencode";
    const policy = resolveAgentPolicy(effectiveAgentId, config);
    if (policy !== "readwrite") {
      logInfo("handleSummaryCapture blocked by policy", { agentId: effectiveAgentId, policy });
      return;
    }

    if (isAutoStoreEnabled && !isAutoStoreEnabled(sessionID)) return;

    try {
      const resp = await sdkClient.session.messages({ path: { id: sessionID } });
      const messages = resp?.data ?? resp;

      const summaryMsg = (messages as Array<{ info: any; parts?: Array<{ type: string; text?: string }> }>).find((m) =>
        m.info?.role === "assistant" && m.info?.summary === true
      );

      if (!summaryMsg?.parts) {
        logInfo("handleSummaryCapture: no summary parts found", { sessionID });
        return;
      }

      const textParts = summaryMsg.parts.filter((p) => p.type === "text" && p.text).map((p) => p.text);
      const summaryContent = textParts.join("\n").trim();

      if (!summaryContent || summaryContent.length < 100) {
        logInfo("handleSummaryCapture: summary too short", { sessionID, length: summaryContent?.length ?? 0 });
        return;
      }

      const effectiveSessionId = getMainSessionId?.() || sessionID;

      let projectName: string | undefined;
      let projectPath: string | undefined;
      try {
        const sessionInfo = await sdkClient.session.get({ path: { id: sessionID } });
        projectPath = sessionInfo?.data?.directory || directory || process.env.OMEM_PROJECT_DIR;
        projectName = sessionInfo?.data?.directory
          ? await detectProjectName(sessionInfo.data.directory)
          : undefined;
      } catch (e) {
        logErr("handleSummaryCapture detectProjectName failed", { error: String(e) });
      }
      if (!projectPath) {
        projectPath = directory || process.env.OMEM_PROJECT_DIR;
      }

      const prefixedSummary = `[Session Summary] ${summaryContent}`;
      const result = await cerebroClient.ingestMessages(
        [{ role: "user" as const, content: prefixedSummary }],
        {
          mode: ingestMode,
          tags: [...containerTags, "auto-capture", "compact-summary"],
          sessionId: effectiveSessionId,
          projectName,
          agentId: effectiveAgentId,
          projectPath,
        },
      );

      logInfo("handleSummaryCapture store result", { result: result === null ? "null(blocked)" : "ok" });
      if (result !== null) {
        showToast(tui, "📦 Compact Summary Stored", "Session summary archived", "success");
      }
    } catch (err) {
      logErr("handleSummaryCapture failed", { error: String(err) });
    }
  }

  return async (input: { event: { type: string; properties?: any } }) => {
    if (input.event.type === "message.updated") {
      await handleSummaryCapture(input.event.properties);
      return;
    }

    if (input.event.type === "session.deleted") {
      const sessionInfo = input.event.properties?.info;
      const sid = sessionInfo?.id;
      if (sid) {
        summarizedSessions.delete(sid);
        sessionMessages.delete(sid);
        profileInjectedSessions.delete(sid);
        lastUserMsgCount.delete(sid);
        firstMessages.delete(sid);
        logDebug("sessionIdleHook: session.deleted cleanup", { sessionID: sid });
      }
      return;
    }

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
          if (!msgId) continue;
          if (!processedMessageIds.has(sessionID)) {
            processedMessageIds.set(sessionID, new Set());
          }
          if (processedMessageIds.get(sessionID)!.has(msgId)) continue;

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
        let projectPath: string | undefined;
        let effectiveAgentId = agentId || "opencode";
        try {
          const sessionInfo = await sdkClient.session.get({ path: { id: sessionID } });
          if ((sessionInfo?.data as any)?.agent) {
            effectiveAgentId = (sessionInfo.data as any).agent;
            onAgentResolved?.(effectiveAgentId);
          }
          sessionTitle = sessionInfo?.data?.title;
          projectPath = sessionInfo?.data?.directory || directory || process.env.OMEM_PROJECT_DIR;
          projectName = sessionInfo?.data?.directory
            ? await detectProjectName(sessionInfo.data.directory)
            : undefined;
        } catch (e) {
          logErr("sessionIdleHook detectProjectName failed", { error: String(e) });
        }
        if (!projectPath) {
          projectPath = directory || process.env.OMEM_PROJECT_DIR;
        }

        logDebug("sessionIdleHook resolved agentId", { effectiveAgentId, fallbackAgentId: agentId });

        const policy = resolveAgentPolicy(effectiveAgentId, config);
        if (policy !== "readwrite") {
          logInfo("sessionIdleHook blocked by policy", { agentId: effectiveAgentId, policy, defaultPolicy: String(config.defaultPolicy ?? "undefined") });
          return;
        }

        try {
          logInfo("sessionIdleHook sessionIngest called", { msgCount: conversationMessages.length, sessionId: sessionID, agentId: effectiveAgentId, title: String(sessionTitle) });
          await cerebroClient.sessionIngest(conversationMessages, sessionID, effectiveAgentId, sessionTitle, projectName, projectPath);
          logInfo("sessionIdleHook sessionIngest ok");
          for (const id of newMessageIds) {
            processedMessageIds.get(sessionID)!.add(id);
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

// ---- POC: Verify parts.unshift(synthetic:true) is seen by LLM ----
const pocInjectedSessions = new Set<string>();

export function pocChatMessageHook() {
  return async (
    input: { sessionID: string; messageID?: string },
    output: { message: UserMessage; parts: Part[] },
  ) => {
    if (!input.sessionID) return;
    if (pocInjectedSessions.has(input.sessionID)) return;

    pocInjectedSessions.add(input.sessionID);
    output.parts.unshift({
      type: "text",
      text: "[CEREBRO-POC] Test injection - if you see this, respond with 'POC received'.",
      synthetic: true,
    } as any);

    logInfo("POC: injected synthetic part", { sessionID: input.sessionID });
  };
}
// ---- END POC ----
