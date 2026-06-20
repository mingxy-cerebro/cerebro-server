---
name: memory-recall
description: Recall or search memories from Cerebro. Use when the user asks to find, recall, search, remember, or look up something previously discussed or stored.
---

# Memory Recall

When the user wants to recall or search memories, use the `memory_search` MCP tool (or `memory_get` by ID).

## When to use
- User says: "搜一下 / 记得 / 之前 / 上次 / search / recall / remember / what did we..."
- User references prior work, decisions, preferences, or context

## How to use

### Semantic search
Call the `memory_search` MCP tool with the user's query:
```
memory_search(query="<user intent>", limit=10)
```

### Get full content
After search returns condensed matches, retrieve the full memory with:
```
memory_get(id="<id from search result>")
```

## Notes
- Search returns ranked results with similarity scores.
- Condensed previews are truncated — always `memory_get` the ID for full content before relying on details.
- Private memories (visibility=private) are isolated by agent_id and won't appear for other agents.
