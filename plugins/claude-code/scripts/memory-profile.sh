#!/usr/bin/env bash
# cerebro Claude Code plugin — memory-profile skill
#
# Retrieve the synthesized user profile (preferences, patterns, identity traits)
# derived from stored memories. Mirrors plugins/opencode/src/tools.ts::memory_profile.
#
# Usage:
#   bash memory-profile.sh
set -euo pipefail

PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-}"
if [[ -z "$PLUGIN_ROOT" ]]; then
  PLUGIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd)" || \
    PLUGIN_ROOT="$(dirname "${BASH_SOURCE[0]:-$0}")/.."
fi
# shellcheck source=../hooks/common.sh
source "${PLUGIN_ROOT}/hooks/common.sh"

pp="$(detect_project_path || true)"

# ─── Build URL-encoded path ──────────────────────────────────────────────────
url_path="$(MEM_PP="$pp" python3 -c '
import os, urllib.parse, sys
pp = (os.environ.get("MEM_PP") or "").strip()
path = "/v2/profile"
if pp:
    path += "?project_path=" + urllib.parse.quote(pp, safe="")
sys.stdout.write(path)
')"

# ─── Fetch + format ──────────────────────────────────────────────────────────
resp="$(omem_get "$url_path")"

printf '%s' "$resp" | python3 -c '
import sys, json
raw = sys.stdin.read()
try:
    data = json.loads(raw)
except Exception:
    print("error: invalid JSON response from server")
    sys.exit(0)
if isinstance(data, dict) and data.get("error"):
    print("error: " + str(data["error"]))
    sys.exit(0)
# Server may return either a bare PreferenceDto[] list or {preferences:[...]}.
if isinstance(data, list):
    prefs = data
elif isinstance(data, dict):
    prefs = data.get("preferences") or data.get("results") or []
else:
    prefs = []
if not prefs:
    print("no profile preferences")
    sys.exit(0)
for p in prefs:
    if not isinstance(p, dict):
        continue
    slot = p.get("slot", "?")
    val = p.get("value", "")
    conf = p.get("confidence", 0)
    scp = p.get("scope", "")
    try:
        line = "%s: %s (conf=%.2f, scope=%s)" % (slot, val, float(conf), scp)
    except (TypeError, ValueError):
        line = "%s: %s (conf=%s, scope=%s)" % (slot, val, conf, scp)
    print(line)
'
