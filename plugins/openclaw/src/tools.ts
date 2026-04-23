import type { OmemClient } from "./client.js";
import type { AnyAgentTool } from "./types.js";

function jsonResult(data: unknown): string {
  if (typeof data === "string") return data;
  try {
    return JSON.stringify(data);
  } catch {
    return String(data);
  }
}

export function buildTools(client: OmemClient): AnyAgentTool[] {
  return [
    {
      name: "memory_store",
      label: "Store Memory",
      description:
        "Store a new memory in the user's long-term memory. " +
        "Use when the user explicitly asks to remember something, " +
        "or when you identify important preferences, facts, or decisions worth preserving.",
      parameters: {
        type: "object",
        properties: {
          content: { type: "string", description: "The information to remember" },
          tags: { type: "array", items: { type: "string" }, description: "Optional categorization tags" },
          source: { type: "string", description: "Origin context, e.g. 'conversation', 'code-review'" },
        },
        required: ["content"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const result = await client.createMemory(
            args.content as string,
            args.tags as string[] | undefined,
            args.source as string | undefined,
          );
          if (!result) return jsonResult({ ok: false, error: "omem server unavailable" });
          return jsonResult({ ok: true, id: result.id, tags: result.tags });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "memory_search",
      label: "Search Memories",
      description:
        "Search the user's long-term memory by semantic similarity. " +
        "Use to recall previously stored preferences, facts, or context.",
      parameters: {
        type: "object",
        properties: {
          query: { type: "string", description: "Natural-language search query" },
          limit: { type: "number", description: "Max results to return (default 10)" },
          scope: { type: "string", description: "Optional scope filter" },
        },
        required: ["query"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const results = await client.searchMemories(
            args.query as string,
            (args.limit as number) ?? 10,
            args.scope as string | undefined,
          );
          if (results.length === 0) return jsonResult({ ok: true, results: [] });
          return jsonResult({
            ok: true,
            results: results.map((r) => ({
              score: r.score,
              id: r.memory.id,
              content: r.memory.content.slice(0, 200),
              tags: r.memory.tags,
              category: r.memory.category,
            })),
          });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "memory_get",
      label: "Get Memory",
      description: "Retrieve a specific memory by its ID.",
      parameters: {
        type: "object",
        properties: {
          id: { type: "string", description: "Memory ID" },
        },
        required: ["id"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const memory = await client.getMemory(args.id as string);
          if (!memory) return jsonResult({ ok: false, error: `Memory ${args.id} not found` });
          return jsonResult({ ok: true, memory });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "memory_update",
      label: "Update Memory",
      description:
        "Update the content or tags of an existing memory. " +
        "Use when information needs correction or enrichment.",
      parameters: {
        type: "object",
        properties: {
          id: { type: "string", description: "Memory ID to update" },
          content: { type: "string", description: "New content" },
          tags: { type: "array", items: { type: "string" }, description: "Replacement tags" },
        },
        required: ["id", "content"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const result = await client.updateMemory(
            args.id as string,
            args.content as string,
            args.tags as string[] | undefined,
          );
          if (!result) return jsonResult({ ok: false, error: `Failed to update memory ${args.id}` });
          return jsonResult({ ok: true, id: args.id });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "memory_delete",
      label: "Delete Memory",
      description: "Delete a memory by ID. Use when the user asks to forget something.",
      parameters: {
        type: "object",
        properties: {
          id: { type: "string", description: "Memory ID to delete" },
        },
        required: ["id"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          await client.deleteMemory(args.id as string);
          return jsonResult({ ok: true, id: args.id });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "space_create",
      label: "Create Space",
      description:
        "Create a shared space (team or organization) for sharing memories across users and agents.",
      parameters: {
        type: "object",
        properties: {
          name: { type: "string", description: "Name of the space" },
          space_type: { type: "string", description: "Type of space: 'team' or 'organization'" },
          members: {
            type: "array",
            items: {
              type: "object",
              properties: {
                user_id: { type: "string", description: "User/tenant ID to add" },
                role: { type: "string", description: "Member role: admin, member, or reader" },
              },
            },
            description: "Initial members to add (optional)",
          },
        },
        required: ["name", "space_type"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const result = await client.createSpace(
            args.name as string,
            args.space_type as string,
            args.members as Array<{ user_id: string; role: string }> | undefined,
          );
          if (!result) return jsonResult({ ok: false, error: "Failed to create space" });
          return jsonResult({ ok: true, space: result });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "space_list",
      label: "List Spaces",
      description:
        "List all spaces you own or are a member of.",
      parameters: {
        type: "object",
        properties: {},
        required: [],
      },
      async execute(_id: string, _params: unknown) {
        try {
          const spaces = await client.listSpaces();
          return jsonResult({ ok: true, spaces });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "space_add_member",
      label: "Add Space Member",
      description:
        "Add a user to an existing shared space with a specified role.",
      parameters: {
        type: "object",
        properties: {
          space_id: { type: "string", description: "Space ID" },
          user_id: { type: "string", description: "User/tenant ID to add" },
          role: { type: "string", description: "Role: admin, member, or reader" },
        },
        required: ["space_id", "user_id", "role"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const result = await client.addSpaceMember(
            args.space_id as string,
            args.user_id as string,
            args.role as string,
          );
          if (!result) return jsonResult({ ok: false, error: "Failed to add member" });
          return jsonResult({ ok: true, result });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "memory_share",
      label: "Share Memory",
      description:
        "Share a memory to a team or organization space. Creates a copy with provenance tracking.",
      parameters: {
        type: "object",
        properties: {
          memory_id: { type: "string", description: "Memory ID to share" },
          target_space: { type: "string", description: "Target space ID" },
        },
        required: ["memory_id", "target_space"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const result = await client.shareMemory(
            args.memory_id as string,
            args.target_space as string,
          );
          if (!result) return jsonResult({ ok: false, error: "Failed to share memory" });
          return jsonResult({ ok: true, result });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "memory_pull",
      label: "Pull Memory",
      description:
        "Pull a shared memory from a team/organization space into your personal space.",
      parameters: {
        type: "object",
        properties: {
          memory_id: { type: "string", description: "Memory ID to pull" },
          source_space: { type: "string", description: "Source space ID" },
          visibility: { type: "string", description: "Visibility of the pulled copy (optional)" },
        },
        required: ["memory_id", "source_space"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const result = await client.pullMemory(
            args.memory_id as string,
            args.source_space as string,
            args.visibility as string | undefined,
          );
          if (!result) return jsonResult({ ok: false, error: "Failed to pull memory" });
          return jsonResult({ ok: true, result });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },

    {
      name: "memory_reshare",
      label: "Reshare Memory",
      description:
        "Refresh a stale shared copy with the latest content and vector from the source memory.",
      parameters: {
        type: "object",
        properties: {
          memory_id: { type: "string", description: "Shared copy memory ID to refresh" },
          target_space: { type: "string", description: "Target space containing the copy (optional)" },
        },
        required: ["memory_id"],
      },
      async execute(_id: string, params: unknown) {
        try {
          const args = (params ?? {}) as Record<string, unknown>;
          const result = await client.reshareMemory(
            args.memory_id as string,
            args.target_space as string | undefined,
          );
          if (!result) return jsonResult({ ok: false, error: "Failed to reshare memory" });
          return jsonResult({ ok: true, result });
        } catch (err) {
          return jsonResult({ ok: false, error: err instanceof Error ? err.message : String(err) });
        }
      },
    },
  ];
}
