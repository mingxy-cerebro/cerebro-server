---
name: apply-dream
description: Review and merge auto dream output. Use when the session-start report says Dream report ready, /dream status shows done, or the user asks to "check the dream result / merge memories". Prints a diff by default; runs --apply only after approval.
---

# apply-dream — dream output review and merge

Script: `$CLAUDE_PLUGIN_ROOT/hooks/apply-dream.mjs` (shipped with the plugin — path never breaks)

## Usage

```bash
# Step 1: read-only review (touches nothing)
node "$CLAUDE_PLUGIN_ROOT/hooks/apply-dream.mjs"

# Step 2: after the user reviews the diff and approves
node "$CLAUDE_PLUGIN_ROOT/hooks/apply-dream.mjs" --apply
```

## Merge contract (built into the script; here for cross-checking)

- join key = name; MEMORY.md index display names (often Chinese) resolve as a second-chance join
- kept entries: old archive wins verbatim; every LLM field except name is ignored
- unknown (unmatched names): surface to the user, never silently dropped
- dropped: listed for review first; deleted only with --apply
- updated/added: LLM content wins, written atomically

## Reporting rule

Summarize the dry run in plain words: kept / updated / added / dropped counts, any unmatched names — wait for the user's go-ahead before --apply.
