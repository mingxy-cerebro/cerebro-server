// Cerebro HTTP client — ported from plugins/opencode/src/client.ts
// Thin fetch wrapper with X-API-Key auth, AbortController timeout, JSON parse.
import { logWarn, logError } from "./logger.js";

function request(baseUrl, apiKey, path, init = {}, timeoutMs) {
  const controller = new AbortController();
  const timeout = setTimeout(
    () => controller.abort(),
    timeoutMs ?? init.__timeoutMs ?? 15000,
  );
  const url = `${baseUrl.replace(/\/+$/, "")}${path}`;
  return fetch(url, {
    ...init,
    signal: controller.signal,
    headers: {
      "Content-Type": "application/json",
      "X-API-Key": apiKey,
      ...(init.headers || {}),
    },
  })
    .then(async (res) => {
      if (!res.ok) {
        const errorBody = await res.text().catch(() => "");
        logWarn("HTTP error", { method: init.method || "GET", path, status: res.status, errorBody });
        throw new Error(`[cerebro] ${res.status} ${res.statusText}${errorBody ? ": " + errorBody : ""}`);
      }
      if (res.status === 204) return null;
      const text = await res.text();
      const trimmed = text.replace(/^\uFEFF/, "").trim();
      if (!trimmed) return null;
      try {
        return JSON.parse(trimmed);
      } catch (parseErr) {
        logError("JSON parse failed", { path, bodyPreview: text.slice(0, 200) });
        throw parseErr;
      }
    })
    .catch((err) => {
      if (err.name === "AbortError") {
        logWarn("Request timed out", { path, timeoutMs: timeoutMs ?? init.__timeoutMs ?? 15000 });
        throw new Error(`[cerebro] Request timed out (${timeoutMs ?? init.__timeoutMs ?? 15000}ms)`);
      }
      throw err;
    })
    .finally(() => clearTimeout(timeout));
}

export class CerebroClient {
  constructor(baseUrl, apiKey, config = {}) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
    this.apiKey = apiKey;
    this.config = config;
  }

  // ── Profile v2 (preferences) ────────────────────────────────────────
  async getInjection(projectPath) {
    const params = projectPath ? `?project_path=${encodeURIComponent(projectPath)}` : "";
    return request(this.baseUrl, this.apiKey, `/v2/profile/inject${params}`, {
      __timeoutMs: 5000,
    });
  }

  async getProfile(projectPath) {
    const params = projectPath ? `?project_path=${encodeURIComponent(projectPath)}` : "";
    return request(this.baseUrl, this.apiKey, `/v2/profile${params}`);
  }

  async getProfileStats() {
    return request(this.baseUrl, this.apiKey, `/v2/profile/stats`);
  }

  // ── Memory CRUD + search ────────────────────────────────────────────
  async listRecent(limit = 20, projectPath) {
    const params = new URLSearchParams({
      limit: String(limit),
      offset: "0",
      sort: "updated_at",
      order: "desc",
    });
    if (projectPath) params.set("project_path", projectPath);
    const res = await request(this.baseUrl, this.apiKey, `/v1/memories?${params}`);
    return res?.memories ?? [];
  }

  async searchMemories(query, limit = 10, scope, tags, projectPath) {
    const maxQ = this.config.content?.maxQueryLength ?? 200;
    const safeQ = (query || "").slice(0, maxQ);
    const params = new URLSearchParams({ q: safeQ, limit: String(limit) });
    if (scope) params.set("scope", scope);
    if (tags && tags.length > 0) params.set("tags", tags.join(","));
    if (projectPath) params.set("project_path", projectPath);
    const res = await request(
      this.baseUrl,
      this.apiKey,
      `/v1/memories/search?${params}`,
      { __timeoutMs: 20000 },
    );
    return res?.results ?? [];
  }

  async getMemory(id) {
    return request(this.baseUrl, this.apiKey, `/v1/memories/${encodeURIComponent(id)}`);
  }

  async createMemory(content, opts = {}) {
    const maxLen = this.config.content?.maxContentLength ?? 3000;
    const safeContent = sanitizeContent(content, maxLen);
    return request(this.baseUrl, this.apiKey, `/v1/memories`, {
      method: "POST",
      body: JSON.stringify({
        content: safeContent,
        tags: opts.tags,
        source: opts.source,
        scope: opts.scope,
        agent_id: opts.agentId,
        session_id: opts.sessionId,
        visibility: opts.visibility,
        category: opts.category,
        project_path: opts.projectPath,
      }),
    });
  }

  async updateMemory(id, content, tags) {
    return request(this.baseUrl, this.apiKey, `/v1/memories/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify({ content, tags }),
    });
  }

  async deleteMemory(id) {
    return request(this.baseUrl, this.apiKey, `/v1/memories/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
  }

  // ── Ingestion ───────────────────────────────────────────────────────
  async ingestMessages(messages, opts = {}) {
    const maxLen = this.config.content?.maxContentLength ?? 3000;
    const safeMessages = messages.map((m) => ({
      role: m.role,
      content: sanitizeContent(m.content, maxLen),
    }));
    return request(this.baseUrl, this.apiKey, `/v1/memories`, {
      method: "POST",
      body: JSON.stringify({
        messages: safeMessages,
        mode: opts.mode ?? "smart",
        agent_id: opts.agentId,
        session_id: opts.sessionId,
        entity_context: opts.entityContext,
        tags: opts.tags,
        project_name: opts.projectName,
        project_path: opts.projectPath,
      }),
    });
  }

  // Session-level ingest (dedicated endpoint, 60s timeout — port of client.ts:386-402)
  async sessionIngest(messages, sessionId, agentId, sessionTitle, projectName, projectPath) {
    return request(
      this.baseUrl,
      this.apiKey,
      `/v1/memories/session-ingest`,
      {
        method: "POST",
        body: JSON.stringify({
          messages,
          session_id: sessionId,
          agent_id: agentId,
          session_title: sessionTitle,
          project_name: projectName,
          project_path: projectPath,
        }),
      },
      60000,
    );
  }

  // ── Stats + recall events ───────────────────────────────────────────
  async getStats() {
    return request(this.baseUrl, this.apiKey, `/v1/stats`);
  }

  async createRecallEvent(params) {
    return request(
      this.baseUrl,
      this.apiKey,
      `/v1/recall-events`,
      { method: "POST", body: JSON.stringify(params) },
      10000,
    );
  }

  // ── Spaces ──────────────────────────────────────────────────────────
  async createSpace(name, spaceType, members) {
    return request(this.baseUrl, this.apiKey, `/v1/spaces`, {
      method: "POST",
      body: JSON.stringify({ name, space_type: spaceType, members }),
    });
  }

  async listSpaces() {
    const res = await request(this.baseUrl, this.apiKey, `/v1/spaces`);
    return res?.spaces ?? [];
  }

  async addSpaceMember(spaceId, userId, role) {
    return request(this.baseUrl, this.apiKey, `/v1/spaces/${encodeURIComponent(spaceId)}/members`, {
      method: "POST",
      body: JSON.stringify({ user_id: userId, role }),
    });
  }

  async shareMemory(memoryId, targetSpace) {
    return request(this.baseUrl, this.apiKey, `/v1/memories/${encodeURIComponent(memoryId)}/share`, {
      method: "POST",
      body: JSON.stringify({ target_space: targetSpace }),
    });
  }

  async pullMemory(memoryId, sourceSpace, visibility) {
    return request(this.baseUrl, this.apiKey, `/v1/memories/${encodeURIComponent(memoryId)}/pull`, {
      method: "POST",
      body: JSON.stringify({ source_space: sourceSpace, visibility }),
    });
  }

  async reshareMemory(memoryId, targetSpace) {
    return request(this.baseUrl, this.apiKey, `/v1/memories/${encodeURIComponent(memoryId)}/reshare`, {
      method: "POST",
      body: JSON.stringify({ target_space: targetSpace }),
    });
  }
}

// avoid circular import at module load: declare local sanitizer
function sanitizeContent(text, maxLen = 3000) {
  if (!text) return "";
  let clean = text.replace(/<[\w-]+[^>]*>[\s\S]*?<\/[\w-]+>/g, "");
  clean = clean.replace(/<[\w-]+[^>]*\/>/g, "");
  clean = clean.replace(/\s+/g, " ").trim();
  if (clean.length <= maxLen) return clean;
  return clean.slice(0, maxLen) + "…[truncated]";
}
