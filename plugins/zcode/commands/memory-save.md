---
description: Save the current session conversation to Cerebro memory (manual archive)
---

The user wants to save the current session to persistent memory NOW.

Do this:
1. Summarize the key decisions, facts, and outcomes from this session so far into concise points.
2. Call the `memory_ingest` tool with those points as messages:
   ```
   memory_ingest(
     messages=[{role:"user", content:"<your concise summary of this session's valuable content>"}],
     mode="smart",
     tags=["manual-save", "zcode"]
   )
   ```

Focus on: decisions made, problems solved, facts learned, preferences discovered.
Skip: trivial chatter, tool outputs, error noise.
Write in the user's language (Chinese if the conversation is in Chinese).
