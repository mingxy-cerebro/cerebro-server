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
}

export interface SearchResult {
  memory: MemoryDto;
  score: number;
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

export interface ShouldRecallResponse {
  should_recall: boolean;
  query?: string;
  reason?: string;
  similarity_score?: number;
  confidence?: number;
  memories?: SearchResult[];
  clustered?: ClusteredRecallResult;
}

export interface SessionRecallRecord {
  session_id: string;
  memory_ids: string[];
  recall_type: string;
  created_at: string;
}

export interface SessionRecallListResponse {
  recalls: SessionRecallRecord[];
}

export interface MemoryDto {
  id: string;
  content: string;
  l2_content?: string;
  category: string;
  memory_type: string;
  state: string;
  tags: string[];
  source?: string;
  tenant_id: string;
  agent_id?: string;
  created_at: string;
  updated_at: string;
}

export class OmemClient {
  constructor(
    private baseUrl: string,
    private apiKey: string,
    private config?: Partial<OmemPluginConfig>,
  ) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
  }

  private getCfg<K extends keyof OmemPluginConfig>(key: K, fallback: OmemPluginConfig[K]): OmemPluginConfig[K] {
    return this.config?.[key] ?? fallback;
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
      timeoutMs ?? this.getCfg("requestTimeoutMs", 15000),
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
        logWarn(`${init.method ?? "GET"} ${path} → ${res.status} ${res.statusText}: ${errorBody}`);
        throw new Error(`[omem] ${res.status} ${res.statusText}${errorBody ? ": " + errorBody : ""}`);
      }

      if (res.status === 204) return null;

      return (await res.json()) as T;
    } catch (err) {
      if ((err as Error).name === "AbortError") {
        logWarn(`${init.method ?? "GET"} ${path} timed out`);
        throw new Error(`[omem] Request timed out (${timeoutMs ?? this.getCfg("requestTimeoutMs", 15000)}ms)`);
      } else {
        logError(`${init.method ?? "GET"} ${path} failed:`, err);
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
  ): Promise<MemoryDto | null> {
    const safeContent = sanitizeContent(content, this.getCfg("maxContentChars", 30000));
    return this.post<MemoryDto>("/v1/memories", {
      content: safeContent,
      tags,
      source,
      scope,
      agent_id: agentId,
      session_id: sessionId,
    });
  }

  async searchMemories(
    query: string,
    limit = 10,
    scope?: string,
    tags?: string[],
  ): Promise<SearchResult[]> {
    const safeQ = truncateQuery(query, this.getCfg("maxQueryLength", 200));
    const params = new URLSearchParams({ q: safeQ, limit: String(limit) });
    if (scope) params.set("scope", scope);
    if (tags && tags.length > 0) params.set("tags", tags.join(","));
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
      content: sanitizeContent(m.content, this.getCfg("maxContentChars", 30000)),
    }));
    return this.post("/v1/memories", {
      messages: safeMessages,
      mode: opts.mode ?? "smart",
      agent_id: opts.agentId,
      session_id: opts.sessionId,
      entity_context: opts.entityContext,
      tags: opts.tags,
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
  ): Promise<ShouldRecallResponse | null> {
    const res = await this.post<ShouldRecallResponse>("/v1/should-recall", {
      query_text,
      last_query_text,
      session_id,
      similarity_threshold,
      max_results,
      project_tags,
    }, 20_000);
    return res;
  }

  async recordSessionRecall(
    session_id: string,
    memory_ids: string[],
    recall_type: string,
    query_text?: string,
    similarity_score?: number,
    llm_confidence?: number,
  ): Promise<unknown | null> {
    const body = {
      session_id,
      memory_ids,
      recall_type,
      query_text: query_text ?? "",
      similarity_score: similarity_score ?? 0,
      llm_confidence: llm_confidence ?? 0,
    };
    const res = await this.post("/v1/session-recalls", body, 20_000);
    return res;
  }

  async listSessionRecalls(
    session_id: string,
  ): Promise<SessionRecallRecord[]> {
    const params = new URLSearchParams({ session_id });
    const res = await this.request<SessionRecallListResponse>(
      `/v1/session-recalls?${params}`,
    );
    return res?.recalls ?? [];
  }
}
