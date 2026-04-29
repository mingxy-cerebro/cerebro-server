import { tool } from "@opencode-ai/plugin";
import type { OmemClient } from "./client.js";
import { isAutoStoreEnabled, setAutoStoreEnabled } from "./index.js";

function extractMemoryIds(result: unknown): string[] {
  if (!result) return [];
  if (Array.isArray(result)) {
    return (result as Array<{ id?: string }>).map((m) => m.id).filter(Boolean) as string[];
  }
  if (typeof result === "object" && result !== null) {
    const r = result as Record<string, unknown>;
    if (Array.isArray(r.memories)) {
      return (r.memories as Array<{ id?: string }>).map((m) => m.id).filter(Boolean) as string[];
    }
    if (Array.isArray(r.results)) {
      return (r.results as Array<{ id?: string; memory?: { id?: string } }>)
        .map((m) => m.id ?? m.memory?.id)
        .filter(Boolean) as string[];
    }
  }
  return [];
}

export interface ToolContext {
  agentId?: string;
  getSessionId: () => string | undefined;
}

export function buildTools(client: OmemClient, containerTags: string[], context: ToolContext) {
  return {
    memory_store: tool({
      description:
        "Store a new memory in the user's long-term memory. " +
        "Use when the user explicitly asks to remember something, " +
        "or when you identify important preferences, facts, or decisions worth preserving.",
      args: {
        content: tool.schema.string().describe("The information to remember"),
        tags: tool.schema
          .array(tool.schema.string())
          .optional()
          .describe("Optional categorization tags"),
        source: tool.schema
          .string()
          .describe("Origin context, e.g. 'conversation', 'code-review', 'user-input'"),
        scope: tool.schema
          .string()
          .optional()
          .describe("Memory scope: 'project' (default, only visible in this project) or 'global' (visible across all projects)"),
        visibility: tool.schema
          .string()
          .optional()
          .describe("Memory visibility: 'global' (default, visible to all agents) or 'private' (only visible to the storing agent). Use 'private' for sensitive data like credentials, personal info, or anything the user wouldn't want shared."),
      },
      async execute(args) {
        const allTags = [...containerTags, ...(args.tags ?? [])];
        const result = await client.createMemory(
          args.content,
          allTags,
          args.source,
          args.scope ?? "project",
          context.agentId,
          context.getSessionId(),
          args.visibility,
        );
        if (!result) return JSON.stringify({ ok: false, error: "The omem server may be unavailable." });
        return JSON.stringify({ ok: true, id: result.id, tags: result.tags });
      },
    }),

    memory_search: tool({
      description:
        "Search the user's long-term memory by semantic similarity. " +
        "Use to recall previously stored preferences, facts, or context.",
      args: {
        query: tool.schema.string().describe("Natural-language search query"),
        limit: tool.schema
          .number()
          .optional()
          .describe("Max results to return (default 10)"),
        scope: tool.schema
          .string()
          .optional()
          .describe("Optional scope filter"),
      },
      async execute(args) {
        const results = await client.searchMemories(
          args.query,
          args.limit ?? 10,
          args.scope,
          containerTags,
        );
        if (results.length === 0) return JSON.stringify({ ok: true, count: 0, results: [] });
        const items = results.map((r) => ({
          id: r.memory.id,
          score: r.score,
          content: r.memory.content.slice(0, 200),
        }));
        return JSON.stringify({ ok: true, count: results.length, results: items });
      },
    }),

    memory_get: tool({
      description: "Retrieve a specific memory by its ID.",
      args: {
        id: tool.schema.string().describe("Memory ID"),
      },
      async execute(args) {
        const memory = await client.getMemory(args.id);
        if (!memory) return JSON.stringify({ ok: false, error: "not found" });
        return JSON.stringify({ ok: true, memory });
      },
    }),

    memory_update: tool({
      description:
        "Update the content or tags of an existing memory. " +
        "Use when information needs correction or enrichment.",
      args: {
        id: tool.schema.string().describe("Memory ID to update"),
        content: tool.schema.string().describe("New content"),
        tags: tool.schema
          .array(tool.schema.string())
          .optional()
          .describe("Replacement tags"),
      },
      async execute(args) {
        const result = await client.updateMemory(
          args.id,
          args.content,
          args.tags,
        );
        if (!result) return JSON.stringify({ ok: false, error: `Failed to update memory ${args.id}` });
        return JSON.stringify({ ok: true, id: args.id });
      },
    }),

    memory_profile: tool({
      description:
        "Get the user profile synthesized from stored memories. Shows preferences, patterns, and key information.",
      args: {},
      async execute() {
        const profile = await client.getProfile();
        if (!profile) return JSON.stringify({ ok: false, error: "Failed to get profile" });
        return JSON.stringify({ ok: true, profile });
      },
    }),

    memory_list: tool({
      description:
        "List the most recent memories. Use to browse what's been remembered without a search query.",
      args: {
        limit: tool.schema
          .number()
          .optional()
          .describe("Max memories to return (default: 20)"),
      },
      async execute(args) {
        const memories = await client.listRecent(args.limit ?? 20);
        if (memories.length === 0) return JSON.stringify({ ok: true, count: 0, memories: [] });
        const items = memories.map((m) => ({
          id: m.id,
          content: m.content.slice(0, 120),
          category: m.category,
          tags: m.tags,
        }));
        return JSON.stringify({ ok: true, count: memories.length, memories: items });
      },
    }),

    session_recalls: tool({
      description:
        "List the injection records for a session. Use to see what memories have been recalled into the current session.",
      args: {
        session_id: tool.schema
          .string()
          .describe("Session ID to query recall records for"),
      },
      async execute(args) {
        const recalls = await client.listSessionRecalls(args.session_id);
        if (recalls.length === 0) return JSON.stringify({ ok: true, count: 0, recalls: [] });
        return JSON.stringify({ ok: true, count: recalls.length, recalls });
      },
    }),

    memory_ingest: tool({
      description:
        "Ingest conversation messages for intelligent extraction. The system extracts atomic facts, deduplicates, and reconciles with existing memories.",
      args: {
        messages: tool.schema
          .array(
            tool.schema.object({
              role: tool.schema.string().describe("Message role: user, assistant, or system"),
              content: tool.schema.string().describe("Message content"),
            }),
          )
          .describe("Conversation messages to ingest"),
        mode: tool.schema
          .enum(["smart", "raw"])
          .optional()
          .describe("Extraction mode: 'smart' (default) or 'raw'"),
        tags: tool.schema
          .array(tool.schema.string())
          .optional()
          .describe("Tags to apply to extracted memories"),
        session_id: tool.schema
          .string()
          .optional()
          .describe("Session ID to associate with the ingestion"),
      },
      async execute(args) {
        const result = await client.ingestMessages(args.messages, {
          mode: args.mode ?? "smart",
          tags: args.tags,
          sessionId: args.session_id,
        });
        if (result === null) return JSON.stringify({ ok: false, error: "Ingestion failed" });
        if (args.session_id) {
          const memoryIds = extractMemoryIds(result);
          if (memoryIds.length > 0) {
            await client.recordSessionRecall(
              args.session_id,
              memoryIds,
              "manual",
              args.messages.map((m) => m.content).join("\n").slice(0, 200),
              0,
              0,
            ).catch(() => {});
          }
        }
        return JSON.stringify({ ok: true, result });
      },
    }),

    memory_stats: tool({
      description:
        "Get statistics about stored memories — counts by category, type, tier, and timeline.",
      args: {},
      async execute() {
        const stats = await client.getStats();
        if (!stats) return JSON.stringify({ ok: false, error: "Failed to get stats" });
        return JSON.stringify({ ok: true, stats });
      },
    }),

    memory_delete: tool({
      description:
        "Delete a memory by ID. Use when the user asks to forget something.",
      args: {
        id: tool.schema.string().describe("Memory ID to delete"),
      },
      async execute(args) {
        try {
          await client.deleteMemory(args.id);
          return JSON.stringify({ ok: true, id: args.id });
        } catch {
          return JSON.stringify({ ok: false, error: `Failed to delete memory ${args.id}` });
        }
      },
    }),

    space_create: tool({
      description:
        "Create a shared space (team or organization) for sharing memories across users and agents.",
      args: {
        name: tool.schema.string().describe("Name of the space"),
        space_type: tool.schema
          .string()
          .describe("Type of space: 'team' or 'organization'"),
        members: tool.schema
          .array(
            tool.schema.object({
              user_id: tool.schema.string().describe("User/tenant ID to add"),
              role: tool.schema.string().describe("Member role: admin, member, or reader"),
            }),
          )
          .optional()
          .describe("Initial members to add"),
      },
      async execute(args) {
        const result = await client.createSpace(
          args.name,
          args.space_type,
          args.members,
        );
        if (!result) return JSON.stringify({ ok: false, error: "Failed to create space" });
        return JSON.stringify({ ok: true, space: result });
      },
    }),

    space_list: tool({
      description:
        "List all spaces you own or are a member of.",
      args: {},
      async execute() {
        const spaces = await client.listSpaces();
        return JSON.stringify({ ok: true, spaces });
      },
    }),

    space_add_member: tool({
      description:
        "Add a user to an existing shared space with a specified role.",
      args: {
        space_id: tool.schema.string().describe("Space ID"),
        user_id: tool.schema.string().describe("User/tenant ID to add"),
        role: tool.schema.string().describe("Role: admin, member, or reader"),
      },
      async execute(args) {
        const result = await client.addSpaceMember(
          args.space_id,
          args.user_id,
          args.role,
        );
        if (!result) return JSON.stringify({ ok: false, error: "Failed to add member" });
        return JSON.stringify({ ok: true, result });
      },
    }),

    memory_share: tool({
      description:
        "Share a memory to a team or organization space. Creates a copy with provenance tracking.",
      args: {
        memory_id: tool.schema.string().describe("Memory ID to share"),
        target_space: tool.schema.string().describe("Target space ID"),
      },
      async execute(args) {
        const result = await client.shareMemory(
          args.memory_id,
          args.target_space,
        );
        if (!result) return JSON.stringify({ ok: false, error: "Failed to share memory" });
        return JSON.stringify({ ok: true, result });
      },
    }),

    memory_pull: tool({
      description:
        "Pull a shared memory from a team/organization space into your personal space.",
      args: {
        memory_id: tool.schema.string().describe("Memory ID to pull"),
        source_space: tool.schema.string().describe("Source space ID"),
        visibility: tool.schema
          .string()
          .optional()
          .describe("Visibility of the pulled copy"),
      },
      async execute(args) {
        const result = await client.pullMemory(
          args.memory_id,
          args.source_space,
          args.visibility,
        );
        if (!result) return JSON.stringify({ ok: false, error: "Failed to pull memory" });
        return JSON.stringify({ ok: true, result });
      },
    }),

    memory_reshare: tool({
      description:
        "Refresh a stale shared copy with the latest content and vector from the source memory.",
      args: {
        memory_id: tool.schema.string().describe("Shared copy memory ID to refresh"),
        target_space: tool.schema
          .string()
          .optional()
          .describe("Target space containing the copy (optional)"),
      },
      async execute(args) {
        const result = await client.reshareMemory(
          args.memory_id,
          args.target_space,
        );
        if (!result) return JSON.stringify({ ok: false, error: "Failed to reshare memory" });
        return JSON.stringify({ ok: true, result });
      },
    }),

    memory_toggle: tool({
      description:
        "Toggle Cerebro auto-store ON or OFF for current session. Does NOT affect manual memory_store calls.",
      args: {
        state: tool.schema
          .string()
          .optional()
          .describe("Set to 'on' or 'off'. Omit to check current status."),
      },
      async execute(args) {
        const sessionId = context.getSessionId();
        if (!sessionId) return JSON.stringify({ ok: false, error: "No active session" });

        const state = args.state?.toLowerCase();
        if (state === "on") {
          setAutoStoreEnabled(sessionId, true);
          return JSON.stringify({ ok: true, auto_store: true, message: "Cerebro auto-store: ON" });
        } else if (state === "off") {
          setAutoStoreEnabled(sessionId, false);
          return JSON.stringify({ ok: true, auto_store: false, message: "Cerebro auto-store: OFF" });
        } else {
          const current = isAutoStoreEnabled(sessionId);
          return JSON.stringify({ ok: true, auto_store: current, message: `Cerebro auto-store: ${current ? "ON" : "OFF"}` });
        }
      },
    }),
  };
}
