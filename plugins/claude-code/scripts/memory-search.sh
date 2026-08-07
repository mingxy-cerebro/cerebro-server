#!/usr/bin/env bash
# cerebro Claude Code plugin — memory-search skill
#
# Semantic search over long-term memory. Mirrors the LLM-facing contract of
# plugins/opencode/src/tools.ts::memory_search but shells out via common.sh.
#
# Usage:
#   bash memory-search.sh "QUERY" [LIMIT]
#   echo "auth flow" | bash memory-search.sh - [LIMIT]
set -euo pipefail

# Locate plugin root (CLAUDE_PLUGIN_ROOT when invoked by Claude Code; otherwise
# derive from this script's location: scripts/foo.sh -> .. = plugin root).
PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-}"
if [[ -z "$PLUGIN_ROOT" ]]; then
  PLUGIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd)" || \
    PLUGIN_ROOT="$(dirname "${BASH_SOURCE[0]:-$0}")/.."
fi
# shellcheck source=../hooks/common.sh
source "${PLUGIN_ROOT}/hooks/common.sh"

# ─── Args ────────────────────────────────────────────────────────────────────
query="${1:-}"
limit="${2:-${MEM_SEARCH_COUNT:-8}}"

# Allow `echo "q" | memory-search.sh - [LIMIT]` for stdin-driven queries.
if [[ "$query" == "-" || (-z "$query" && ! -t 0) ]]; then
  query=$(cat || true)
fi

if [[ -z "$query" ]]; then
  echo "usage: memory-search.sh \"QUERY\" [LIMIT]" >&2
  exit 1
fi

query="$(truncate_query "$query")"
tags="$(container_tags || true)"
pp="$(detect_project_path || true)"

# ─── Build URL-encoded GET path via python3 (avoid bash quoting traps) ───────
url_path="$(MEM_Q="$query" MEM_TAGS="$tags" MEM_PP="$pp" MEM_LIMIT="$limit" python3 -c '
import os, urllib.parse, sys
q = os.environ.get("MEM_Q", "")
limit = os.environ.get("MEM_LIMIT", "8")
parts = [
    "q=" + urllib.parse.quote(q, safe=""),
    "limit=" + urllib.parse.quote(str(limit), safe=""),
]
tags_raw = (os.environ.get("MEM_TAGS") or "").strip()
if tags_raw:
    comma = ",".join(t for t in tags_raw.split() if t)
    if comma:
        # comma is reserved-safe in query per RFC 3986; keep literal
        parts.append("tags=" + ",".join(urllib.parse.quote(t, safe="") for t in comma.split(",")))
pp = (os.environ.get("MEM_PP") or "").strip()
if pp:
    parts.append("project_path=" + urllib.parse.quote(pp, safe=""))
sys.stdout.write("/v1/memories/search?" + "&".join(parts))
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
results = data.get("results", []) if isinstance(data, dict) else []
if not results:
    print("no memories")
    sys.exit(0)
for r in results:
    if not isinstance(r, dict):
        continue
    m = r.get("memory") or {}
    if not isinstance(m, dict):
        m = {}
    score = r.get("score", 0.0)
    mid = m.get("id", "?")
    content = (m.get("content") or "")
    snippet = content[:200]
    try:
        line = "[%.2f] %s: %s" % (float(score), mid, snippet)
    except (TypeError, ValueError):
        line = "[%s] %s: %s" % (score, mid, snippet)
    print(line)
'
