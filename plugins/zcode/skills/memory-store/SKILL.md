---
name: memory-store
description: Store or remember something in Cerebro persistent memory. Use when the user says remember, save, store, don't forget, 记住, 保存, 别忘了.
---

# Memory Store

When the user wants you to remember something, use the `memory_store` MCP tool.

## When to use
- User says: "记住 / 保存 / 别忘了 / remember / save this / store / don't forget..."
- Important decision, preference, fact, or context worth persisting across sessions

## How to use

Call the `memory_store` MCP tool:
```
memory_store(
  content="<concise factual statement>",
  tags=["<category tag>"],
  scope="project" | "global",
  visibility="global" | "private",
  category="cases" | "preferences" | "entities" | "events" | "profile" | "patterns"
)
```

## Categorization guide
- **cases** — debugging experience, troubleshooting steps, lessons learned
- **preferences** — user's stated preferences (tools, style, workflow)
- **entities** — people, projects, systems, APIs (with stable identity)
- **events** — dated occurrences (releases, incidents, milestones)
- **profile** — core identity/background info about the user
- **patterns** — recurring patterns or conventions

## Scope & visibility
- `scope=project` — relevant only to current project (default for project work)
- `scope=global` — cross-project knowledge
- `visibility=private` — **use for passwords, API keys, personal data** — isolated by agent_id

## Notes
- Write concise factual statements, not verbose narratives.
- One fact per memory when possible.
- The content is auto-sanitized (XML stripped, whitespace compressed, truncated to 3000 chars).
