---
name: memory-recall
description: Search and recall memories from Cerebro. Use when user asks to find, recall, search, or remember something.
---

# Memory Recall

Search Cerebro for relevant memories using semantic search.

## How to search

```bash
curl -sf \
  -H "X-API-Key: $OMEM_API_KEY" \
  -H "Accept: application/json" \
  "${OMEM_API_URL:-http://localhost:8080}/v1/memories/search?q=$ARGUMENTS&limit=10"
```

Replace `$ARGUMENTS` with the URL-encoded search query.

## Response Format

The API returns:
```json
{
  "results": [
    {
      "memory": {
        "id": "...",
        "content": "...",
        "tags": ["..."],
        "created_at": "..."
      },
      "score": 0.95
    }
  ]
}
```
