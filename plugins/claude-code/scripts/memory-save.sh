#!/usr/bin/env bash
# cerebro Claude Code plugin — memory-save skill
#
# Persist a fact / decision / preference to long-term memory. Mirrors the
# LLM-facing contract of plugins/opencode/src/tools.ts::memory_store.
#
# Usage:
#   bash memory-save.sh --content "..." [--tags "t1,t2"] [--category X] \
#                       [--visibility global|private] [--scope project|global]
#   echo "..." | bash memory-save.sh            # content from stdin
#
# Category enum (lowercase): cases | preferences | entities | events | profile | patterns
set -euo pipefail

PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-}"
if [[ -z "$PLUGIN_ROOT" ]]; then
  PLUGIN_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd)" || \
    PLUGIN_ROOT="$(dirname "${BASH_SOURCE[0]:-$0}")/.."
fi
# shellcheck source=../hooks/common.sh
source "${PLUGIN_ROOT}/hooks/common.sh"

# ─── Arg parsing ─────────────────────────────────────────────────────────────
content=""
tags_arg=""
category=""
visibility="global"
scope="project"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --content)    content="${2:-}"; shift 2 ;;
    --tags)       tags_arg="${2:-}"; shift 2 ;;
    --category)   category="${2:-}"; shift 2 ;;
    --visibility) visibility="${2:-}"; shift 2 ;;
    --scope)      scope="${2:-}"; shift 2 ;;
    --help|-h)
      sed -n '2,15p' "${BASH_SOURCE[0]:-$0}" >&2; exit 0 ;;
    --*)          echo "unknown option: $1" >&2; exit 2 ;;
    *)            content="$1"; shift ;;
  esac
done

# stdin fallback when no --content / positional given
if [[ -z "$content" && ! -t 0 ]]; then
  content="$(cat || true)"
fi

if [[ -z "$content" ]]; then
  echo 'usage: memory-save.sh --content "..." [--tags t1,t2] [--category X] [--visibility global|private] [--scope project|global]' >&2
  exit 1
fi

# Basic enum validation (fail fast with a friendly message, do not crash harder).
case "$visibility" in
  global|private) : ;;
  *) echo "error: --visibility must be 'global' or 'private' (got: $visibility)" >&2; exit 2 ;;
esac
case "$scope" in
  project|global) : ;;
  *) echo "error: --scope must be 'project' or 'global' (got: $scope)" >&2; exit 2 ;;
esac
if [[ -n "$category" ]]; then
  case "$category" in
    cases|preferences|entities|events|profile|patterns) : ;;
    *) echo "error: --category must be one of cases|preferences|entities|events|profile|patterns (got: $category)" >&2; exit 2 ;;
  esac
fi

# ─── Sanitize + assemble tags + project_path ─────────────────────────────────
content="$(printf '%s' "$content" | sanitize_content)"
container="$(container_tags || true)"
pp="$(detect_project_path || true)"

# ─── Build JSON body via python3 ─────────────────────────────────────────────
body="$(MEM_CONTENT="$content" MEM_CONTAINER="$container" MEM_TAGS="$tags_arg" \
        MEM_CAT="$category" MEM_VIS="$visibility" MEM_SCOPE="$scope" MEM_PP="$pp" \
        python3 -c '
import os, json, sys
content = os.environ["MEM_CONTENT"]
container = (os.environ.get("MEM_CONTAINER") or "").split()
user_tags = [t.strip() for t in (os.environ.get("MEM_TAGS") or "").split(",") if t.strip()]
tags = container + user_tags
body = {
    "content": content,
    "tags": tags,
    "source": "claude-code",
    "scope": os.environ.get("MEM_SCOPE") or "project",
    "agent_id": "cerebro",
    "visibility": os.environ.get("MEM_VIS") or "global",
}
pp = (os.environ.get("MEM_PP") or "").strip()
if pp:
    body["project_path"] = pp
cat = (os.environ.get("MEM_CAT") or "").strip()
if cat:
    body["category"] = cat
sys.stdout.write(json.dumps(body, ensure_ascii=False))
')"

# ─── POST + format response ──────────────────────────────────────────────────
resp="$(omem_post "/v1/memories" "$body")"

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
if not isinstance(data, dict):
    print("ok")
    sys.exit(0)
mid = data.get("id")
extra = ""
tags = data.get("tags") or []
if isinstance(tags, list) and tags:
    extra = " tags=" + ",".join(str(t) for t in tags)
if mid is not None:
    print("ok id=" + str(mid) + extra)
else:
    print("ok" + extra)
'
