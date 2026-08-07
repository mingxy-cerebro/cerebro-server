---
name: memory-profile
description: Retrieve the synthesized user profile (preferences, patterns, identity traits) induced from stored memories. Use at session start to ground yourself in the user's working style, tooling preferences, recurring workflows, and role context; or whenever adapting tone, format, or approach to match the user's established patterns would improve the interaction.
---

# Memory Profile

Read the induced preference profile for the current user/project. Each preference is a `{slot, value, confidence, scope}` tuple synthesized from raw memories.

## When to use

- Session bootstrap — load working-style context before doing work
- User asks "what do you know about me / my preferences / how I work"
- Adapting response format, tone, or tooling to the user's established patterns
- Checking whether a candidate action aligns with stored preferences

## How to run

```bash
bash "$CLAUDE_PLUGIN_ROOT/scripts/memory-profile.sh"
```

No arguments. `project_path` is auto-detected from git toplevel and forwarded to the server.

## Output format

One line per preference:

```
preferred_language: rust (conf=0.92, scope=global)
review_style: terse, no praise (conf=0.85, scope=project)
indent_style: 2-space (conf=0.78, scope=global)
```

`no profile preferences` when the profile is empty. `error: …` on transport/server failure.

## Notes

- Confidence (`conf`) is the induction strength — treat values below ~0.5 as weak signals.
- `scope=global` preferences apply across projects; `scope=project` only to the current one.
- Profile is read-only here. To influence it, save memories via `memory-save` with `category=preferences` or `category=profile`; the server's induction pipeline will fold them in.
