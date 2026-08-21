---
name: dream
description: Control the auto dream switch and status, or dream right now. Use when the user says /dream on, /dream off, /dream status, /dream now, or "dream switch / dream status / enable / disable / dream now".
---

# /dream — auto dream master switch

Args: `on` | `off` | `status` | `now` (default `status`).

## Config file

`~/.cache/cerebro/dream/config.json`:

```json
{"enabled": true, "badge_ttl_secs": 3600}
```

- `enabled`: master switch. When false, every trigger path (session-end hook / session-start fallback / systemd timer) exits immediately — no dreaming.
- `badge_ttl_secs`: how long (seconds) the red fail badge stays on the statusline before falling back to the accumulating state.

## on / off

Rewrite the whole file with the Write tool (Bash sandbox has a read-only home — no echo/jq redirects). Change only the `enabled` field, keep the rest verbatim. Then verify:

```bash
node "$CLAUDE_PLUGIN_ROOT/hooks/dream.mjs" --badge
```

Expected: off prints `{"text":"cerebro dream off","color":90}`; on returns to the normal state.

## status

Report four items in plain words for the user (translate, no raw JSON):

1. **Badge**: run `--badge` above and translate:
   - `off` grey = disabled
   - `run` orange = dreaming right now
   - `fail` red = last dream failed (check `error` in state)
   - `done·apply` green = a dream result sits unconsumed — suggest `/apply-dream`
   - `rdy` green = material ready, waiting for a trigger
   - `[n/2]·Xh Ym` = accumulating: n new sessions since the last dream, Xh Ym left to the 6h window
2. **State**: `jq '{phase, last_dream_at, error, consumed}' ~/.cache/cerebro/dream/state.json`
3. **Timer**: `systemctl --user list-timers cerebro-dream.timer --no-pager | head -3` (sandbox blocks D-Bus with "Failed to connect to bus" — rerun via a host channel like `wsl.exe -- bash -lc`, or ask the user to run it with the `!` prefix)
4. **Output**: `ls -t ~/.cache/cerebro/dream/output/ | head -3`; to consume a result suggest `/apply-dream` (diff only by default, writes after approval).

## now

Dream immediately, skipping the 6h+2-session gates (manual override). Still respects the off switch and the concurrency lock. Run detached — a dream polls the server for up to 11 minutes:

```bash
nohup node "$CLAUDE_PLUGIN_ROOT/hooks/dream.mjs" --now >/dev/null 2>&1 &
```

Tell the user it's running (badge turns `run` orange). Check back with `/dream status` in a few minutes; when done suggest `/apply-dream` to review the diff.
