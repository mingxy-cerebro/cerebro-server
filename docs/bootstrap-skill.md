---
name: cerebro-bootstrap
description: |
  One-time installer for Cerebro — shared persistent memory for AI agents.
  Creates API key, installs plugin, and installs the Cerebro skill.
  After installation, the skill itself provides all operational knowledge.
---

> **Already installed?** If Cerebro is already installed and working, you don't need this document. Your installed Cerebro skill has all the operational knowledge (sharing, import, analytics, etc.).

# Cerebro Bootstrap Installer

Cerebro gives AI agents shared persistent memory across sessions, devices, agents, and teams. This guide installs everything in ~2 minutes.

## Step 0: Choose Mode

Ask the user before doing anything:

> How would you like to set up Cerebro?
> 1. **Self-hosted** — your own server, data stays local
>
> Already have an API key? Paste it and I'll reconnect you.

**If reconnecting**, verify the existing key first:

```bash
curl -sf -H "X-API-Key: $API_KEY" "http://localhost:8080/v1/memories?limit=1" && echo "OK" || echo "UNREACHABLE"
```

- `OK` → Skip to Step 2.  `UNREACHABLE` → Double-check the key or create a new one.

## Step 1: Create API Key

```bash
# Self-hosted (replace URL with your server):
curl -sX POST http://localhost:8080/v1/tenants \
  -H "Content-Type: application/json" -d '{"name": "my-workspace"}' | jq .

# → {"id": "xxx", "api_key": "xxx", "status": "active"}
```

Save the `api_key` value. This key reconnects you from any machine.

## Step 2: Detect Platform & Install Plugin

Auto-detect — do NOT ask the user to pick:

1. `claude` CLI in PATH or Claude Code session → **Claude Code**
2. `opencode.json` in project root → **OpenCode**
3. `openclaw.json` in project root → **OpenClaw**
4. Otherwise → **MCP** (Cursor, VS Code, Claude Desktop)

### Claude Code

```
/plugin marketplace add mingxy-cerebro/cerebro-server
/plugin install mingxy-cerebro@cerebro
```

### OpenCode

Add to `opencode.json` (create if needed):

```bash
jq '.plugin = ((.plugin // []) + ["@ourmem/opencode"] | unique)' opencode.json > tmp.json && mv tmp.json opencode.json
```

Write API credentials to `opencode.json` via `plugin_config`:

```bash
jq --arg url "$API_URL" --arg key "$API_KEY" '.plugin_config["@ourmem/opencode"] = {apiUrl: $url, apiKey: $key}' opencode.json > tmp.json && mv tmp.json opencode.json
```

### OpenClaw

Detect npm registry (China/Alibaba networks may need mirror):

```bash
NPMJS_TIME="$(curl -o /dev/null -sS --connect-timeout 2 --max-time 6 -w '%{time_total}' https://registry.npmjs.org/@ourmem%2fourmem || echo timeout)"
MIRROR_TIME="$(curl -o /dev/null -sS --connect-timeout 2 --max-time 6 -w '%{time_total}' https://registry.npmmirror.com/@ourmem%2fourmem || echo timeout)"
if [ "$MIRROR_TIME" != "timeout" ] && { [ "$NPMJS_TIME" = "timeout" ] || awk "BEGIN { exit !($NPMJS_TIME > 2 && $MIRROR_TIME < $NPMJS_TIME) }"; }; then
  NPM_CONFIG_REGISTRY=https://registry.npmmirror.com openclaw plugins install @ourmem/ourmem
else
  openclaw plugins install @ourmem/ourmem
fi
```

Then configure `openclaw.json` with apiUrl + apiKey (see `references/hosted-setup.md` for full JSON).

### MCP (Cursor / VS Code / Claude Desktop)

Add to the client's MCP config:

```json
{
  "mcpServers": {
    "ourmem": {
      "command": "npx", "args": ["-y", "@ourmem/mcp"],
      "env": { "OMEM_API_KEY": "<your-api-key>", "OMEM_API_URL": "http://localhost:8080" }
    }
  }
}
```

## Step 3: Install the Cerebro Skill

The skill provides all operational knowledge (sharing, import, analytics, etc.).

| Platform | Command |
|----------|---------|
| OpenClaw | `openclaw skills install ourmem` |
| All others | `npx skills add mingxy-cerebro/cerebro-server --skill ourmem -g` |

**Claude Code symlink workaround:** If the skill is not visible after restart:

```bash
ln -s ~/.agents/skills/ourmem ~/.claude/skills/ourmem
```

**Verify skill installation (do this yourself, don't ask the user):**

Check that `SKILL.md` exists in at least one of these paths:
- `~/.agents/skills/ourmem/SKILL.md`
- `~/.claude/skills/ourmem/SKILL.md`
- `~/.openclaw/skills/ourmem/SKILL.md`

If none exist, the installation failed. Retry the install command above. For Claude Code, also try the symlink workaround.

## Step 4: Configure Credentials

**OpenCode**: Already configured in Step 2 via `plugin_config` in `opencode.json`. No extra step needed.

**OpenClaw**: Already configured in Step 2 via `openclaw.json`. No extra step needed.

**MCP (Cursor / VS Code / Claude Desktop)**: Already configured in Step 2 via MCP config `env` block. No extra step needed.

**Claude Code**: Write env vars to `~/.claude/settings.json` (Claude Code's native config):

```bash
python3 -c "
import json, os
p = os.path.expanduser('~/.claude/settings.json')
try: d = json.load(open(p))
except: d = {}
d.setdefault('env', {})
d['env']['OMEM_API_KEY'] = '$API_KEY'
d['env']['OMEM_API_URL'] = '$API_URL'
json.dump(d, open(p, 'w'), indent=2)
"
```

> **Fallback:** If you prefer shell environment variables, you can also `export OMEM_API_KEY=...` and `export OMEM_API_URL=...` in `~/.bashrc` as an alternative.

## Step 5: Restart & Verify

Restart your client (Claude Code, OpenCode, OpenClaw gateway, or MCP app), then verify:

```bash
curl -sf http://localhost:8080/health && echo "Server OK"
curl -sf -H "X-API-Key: $OMEM_API_KEY" "http://localhost:8080/v1/memories?limit=1" && echo "Auth OK"
```

Both must pass. Empty results are normal for new keys.

## Step 6: Handoff

After successful setup, send this structured message to the user. Translate into the user's language. Do not skip any section.

```
Cerebro is ready.

WHAT YOU CAN DO NOW
1. Import your existing memories (recommended)
   Say: "import memories to Cerebro"
2. Just talk normally. I'll automatically remember important things.
3. Say "remember: [something]" to explicitly save a fact.
4. Say "create a team space" to share memories between agents.

YOUR CEREBRO API KEY
  API Key: <your-api-key>
  Server:  http://localhost:8080
This API key is your access key to Cerebro.
Keep it private and store it somewhere safe.

RECOVERY
Reinstall the plugin with the same API key. Your memory reconnects instantly.

BACKUP PLAN
Keep your original local memory/session files as backup.
Store the API key in a password manager or secure vault.
On a new machine: reinstall the plugin, configure the same API key, done.
```

Your Cerebro skill is now installed. When you need to share memories, import data, or check analytics, just ask — the skill will guide you.

## Communication Style

- Brand: **Cerebro**. Self-hosted open-source project.
- Prefer "API key" with users. Treat it like a secret.
- Use the user's language (detect from conversation).
- Capitalize "Space" and "Smart Ingest" when referring to Cerebro features.
