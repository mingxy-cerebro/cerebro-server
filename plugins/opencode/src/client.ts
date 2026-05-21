import { logWarn, logError } from "./logger.js";
import type { OmemPluginConfig } from "./config.js";

function sanitizeContent(text: string, maxLen: number): string {
  let clean = text.replace(/<[\w-]+[^>]*>[\s\S]*?<\/[\w-]+>/g, "");
  clean = clean.replace(/<[\w-]+[^>]*\/>/g, "");
  clean = clean.replace(/\s+/g, " ").trim();
  if (clean.length <= maxLen) return clean;
  return clean.slice(0, maxLen) + "…[truncated]";
}

function truncateQuery(query: string, maxLen: number): string {
  if (query.length <= maxLen) return query;
  return query.slice(0, maxLen);
}

export interface IngestOptions {
  mode?: "smart" | "raw";
  agentId?: string;
  sessionId?: string;
  entityContext?: string;
  tags?: string[];
  projectName?: string;
  projectPath?: string;
}

export interface SearchResult {
  memory: MemoryDto;
  score: number;
  refine_relevance?: string;
  refine_reasoning?: string;
}

export interface SearchResponse {
  results: SearchResult[];
  trace?: unknown;
}

export interface ListResponse {
  memories: MemoryDto[];
  limit: number;
  offset: number;
}

export interface ClusterSummary {
  cluster_id: string;
  title: string;
  summary: string;
  member_count: number;
  relevance_score: number;
  key_memories: MemoryDto[];
}

export interface ClusteredRecallResult {
  cluster_summaries: ClusterSummary[];
  standalone_memories: MemoryDto[];
}

export interface DiscardedItem {
  memory_id: string;
  content: string;
  score: number;
  refine_relevance?: string;
  refine_reasoning?: string;
}

export interface ShouldRecallResponse {
  should_recall: boolean;
  query?: string;
  reason?: string;
  similarity_score?: number;
  confidence?: number;
  memories?: SearchResult[];
  discarded?: DiscardedItem[];
  clustered?: ClusteredRecallResult;
}

export interface MemoryRelation {
  relation_type: string;
  target_id: string;
  context_label?: string;
}

export interface MemoryDto {
  id: string;
  content: string;
  l2_content?: string;
  category: string;
  memory_type: string;
  state: string;
  tags: string[];
  relations?: MemoryRelation[];
  source?: string;
  tenant_id: string;
  agent_id?: string;
  importance: number;
  created_at: string;
  updated_at: string;
}

export class CerebroClient {
  constructor(
    private baseUrl: string,
    private apiKey: string,
    private config?: Partial<OmemPluginConfig>,
  ) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
  }

  private getCfg<S extends keyof OmemPluginConfig, K extends string & keyof OmemPluginConfig[S]>(
    section: S, key: K, fallback: OmemPluginConfig[S][K],
  ): OmemPluginConfig[S][K] {
    const sec = this.config?.[section] as Record<string, unknown> | undefined;
    return (sec?.[key] ?? fallback) as OmemPluginConfig[S][K];
  }

  private async request<T>(
    path: string,
    init: RequestInit = {},
    timeoutMs?: number,
  ): Promise<T | null> {
    const url = `${this.baseUrl}${path}`;
    const controller = new AbortController();
    const timeout = setTimeout(
      () => controller.abort(),
      timeoutMs ?? this.getCfg("connection", "requestTimeoutMs", 15000),
    );

    try {
      const res = await fetch(url, {
        ...init,
        signal: controller.signal,
        headers: {
          "Content-Type": "application/json",
          "X-API-Key": this.apiKey,
          ...(init.headers as Record<string, string>),
        },
      });

      if (!res.ok) {
        const errorBody = await res.text().catch(() => "");
        logWarn("HTTP error", { method: init.method ?? "GET", path, status: res.status, statusText: res.statusText, errorBody });
        throw new Error(`[cerebro] ${res.status} ${res.statusText}${errorBody ? ": " + errorBody : ""}`);
      }

      if (res.status === 204) return null;

      return (await res.json()) as T;
    } catch (err) {
      if ((err as Error).name === "AbortError") {
        logWarn("Request timed out", { method: init.method ?? "GET", path, timeoutMs: timeoutMs ?? this.getCfg("connection", "requestTimeoutMs", 15000) });
        throw new Error(`[cerebro] Request timed out (${timeoutMs ?? this.getCfg("connection", "requestTimeoutMs", 15000)}ms)`);
      } else {
        logError("Request failed", { method: init.method ?? "GET", path, error: String(err) });
        throw err;
      }
    } finally {
      clearTimeout(timeout);
    }
  }

  private post<T>(path: string, body: unknown, timeoutMs?: number): Promise<T | null> {
    return this.request<T>(path, {
      method: "POST",
      body: JSON.stringify(body),
    }, timeoutMs);
  }

  private put<T>(path: string, body: unknown): Promise<T | null> {
    return this.request<T>(path, {
      method: "PUT",
      body: JSON.stringify(body),
    });
  }

  private patch<T>(path: string, body: unknown, timeoutMs?: number): Promise<T | null> {
    return this.request<T>(path, {
      method: "PATCH",
      body: JSON.stringify(body),
    }, timeoutMs);
  }

  private del<T>(path: string): Promise<T | null> {
    return this.request<T>(path, { method: "DELETE" });
  }

  async createMemory(
    content: string,
    tags?: string[],
    source?: string,
    scope?: string,
    agentId?: string,
    sessionId?: string,
    visibility?: string,
    category?: string,
    projectPath?: string,
  ): Promise<MemoryDto | null> {
    const safeContent = sanitizeContent(content, this.getCfg("content", "maxContentChars", 30000));
    return this.post<MemoryDto>("/v1/memories", {
      content: safeContent,
      tags,
      source,
      scope,
      agent_id: agentId,
      session_id: sessionId,
      visibility,
      category,
      project_path: projectPath,
    });
  }

  async searchMemories(
    query: string,
    limit = 10,
    scope?: string,
    tags?: string[],
    projectPath?: string,
  ): Promise<SearchResult[]> {
    const safeQ = truncateQuery(query, this.getCfg("content", "maxQueryLength", 200));
    const params = new URLSearchParams({ q: safeQ, limit: String(limit) });
    if (scope) params.set("scope", scope);
    if (tags && tags.length > 0) params.set("tags", tags.join(","));
    if (projectPath) params.set("project_path", projectPath);
    const res = await this.request<SearchResponse>(
      `/v1/memories/search?${params}`,
      {},
      20_000,
    );
    return res?.results ?? [];
  }

  async getMemory(id: string): Promise<MemoryDto | null> {
    return this.request<MemoryDto>(`/v1/memories/${encodeURIComponent(id)}`);
  }

  async updateMemory(
    id: string,
    content: string,
    tags?: string[],
  ): Promise<MemoryDto | null> {
    return this.put<MemoryDto>(
      `/v1/memories/${encodeURIComponent(id)}`,
      { content, tags },
    );
  }

  async deleteMemory(id: string): Promise<void> {
    await this.del(`/v1/memories/${encodeURIComponent(id)}`);
  }

  async ingestMessages(
    messages: Array<{ role: string; content: string }>,
    opts: IngestOptions = {},
  ): Promise<unknown> {
    const safeMessages = messages.map(m => ({
      role: m.role,
      content: sanitizeContent(m.content, this.getCfg("content", "maxContentChars", 30000)),
    }));
    return this.post("/v1/memories", {
      messages: safeMessages,
      mode: opts.mode ?? "smart",
      agent_id: opts.agentId,
      session_id: opts.sessionId,
      entity_context: opts.entityContext,
      tags: opts.tags,
      project_name: opts.projectName,
      project_path: opts.projectPath,
    });
  }

  async getProfile(_query?: string): Promise<unknown> {
    return this.request("/v1/profile");
  }

  async getStats(): Promise<unknown> {
    return this.request("/v1/stats");
  }

  async listRecent(limit = 20): Promise<MemoryDto[]> {
    const res = await this.request<ListResponse>(
      `/v1/memories?limit=${limit}&offset=0`,
    );
    return res?.memories ?? [];
  }

  async createSpace(
    name: string,
    spaceType: string,
    members?: Array<{ user_id: string; role: string }>,
  ): Promise<unknown> {
    return this.post("/v1/spaces", { name, space_type: spaceType, members });
  }

  async listSpaces(): Promise<unknown[]> {
    const res = await this.request<{ spaces: unknown[] }>("/v1/spaces");
    return res?.spaces ?? [];
  }

  async addSpaceMember(
    spaceId: string,
    userId: string,
    role: string,
  ): Promise<unknown> {
    return this.post(
      `/v1/spaces/${encodeURIComponent(spaceId)}/members`,
      { user_id: userId, role },
    );
  }

  async shareMemory(
    memoryId: string,
    targetSpace: string,
  ): Promise<unknown> {
    return this.post(
      `/v1/memories/${encodeURIComponent(memoryId)}/share`,
      { target_space: targetSpace },
    );
  }

  async pullMemory(
    memoryId: string,
    sourceSpace: string,
    visibility?: string,
  ): Promise<unknown> {
    return this.post(
      `/v1/memories/${encodeURIComponent(memoryId)}/pull`,
      { source_space: sourceSpace, visibility },
    );
  }

  async reshareMemory(
    memoryId: string,
    targetSpace?: string,
  ): Promise<unknown> {
    return this.post(
      `/v1/memories/${encodeURIComponent(memoryId)}/reshare`,
      { target_space: targetSpace },
    );
  }

  async shouldRecall(
    query_text: string,
    last_query_text: string | undefined,
    session_id: string,
    similarity_threshold?: number,
    max_results?: number,
    project_tags?: string[],
    conversation_context?: string[],
    recall_overrides?: {
      fetch_multiplier?: number;
      topk_cap_multiplier?: number;
      mmr_jaccard_threshold?: number;
      mmr_penalty_factor?: number;
      phase2_multiplier?: number;
      llm_max_eval?: number;
      refine_strategy?: string;
      refine_medium_chars?: number;
      skip_llm_gate?: boolean;
    },
    projectPath?: string,
  ): Promise<ShouldRecallResponse | null> {
    const res = await this.post<ShouldRecallResponse>("/v1/should-recall", {
      query_text,
      last_query_text,
      session_id,
      similarity_threshold,
      max_results,
      project_tags,
      conversation_context,
      ...recall_overrides,
      project_path: projectPath,
    }, 20_000);
    return res;
  }

  async updateProfileInjected(
    event_id: string,
    profile_injected: boolean,
    profile_content?: string,
  ): Promise<unknown | null> {
    const body: Record<string, unknown> = { profile_injected };
    if (profile_content !== undefined) {
      body.profile_content = profile_content;
    }
    const res = await this.patch(
      `/v1/recall-events/${event_id}/profile-injected`,
      body,
      10_000,
    );
    return res;
  }

  async createRecallEvent(params: {
    session_id: string;
    recall_type?: string;
    query_text: string;
    max_score: number;
    llm_confidence: number;
    profile_injected: boolean;
    kept_count: number;
    discarded_count: number;
    injected_count: number;
    profile_content?: string;
    injected_content?: string;
    items?: Array<{
      memory_id: string;
      score: number;
      refine_relevance?: string;
      refine_reasoning?: string;
      is_kept: boolean;
    }>;
  }): Promise<{ ok: boolean; event_id?: string } | null> {
    return this.post("/v1/recall-events", params, 10_000);
  }

  async sessionIngest(
    messages: Array<{ role: string; content: string }>,
    sessionId?: string,
    agentId?: string,
    sessionTitle?: string,
    projectName?: string,
    projectPath?: string,
  ): Promise<unknown> {
    return this.post("/v1/memories/session-ingest", {
      messages,
      session_id: sessionId,
      agent_id: agentId,
      session_title: sessionTitle,
      project_name: projectName,
      project_path: projectPath,
    }, 60000);
  }
}
