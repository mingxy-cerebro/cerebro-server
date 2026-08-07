#!/usr/bin/env bash
# cerebro Claude Code plugin — shared HTTP + utility base (Phase1)
#
# Backward-compat guarantees (do NOT break existing hooks):
#   - env vars:  OMEM_API_URL, OMEM_API_KEY
#   - functions: omem_get, omem_post, read_stdin   (signatures unchanged)
# Phase1 adds: load_cerebro_config, omem_put/patch/delete, omem_health,
#   detect_project_path, container_tags, sanitize_content, truncate_query,
#   read_hook_input, log_*.
#
# Config cascade (mirrors plugins/opencode/src/config.ts — env wins):
#   builtin default  <  ~/.config/cerebro/config.json  <  env var
set -euo pipefail

# ─── Builtin defaults ────────────────────────────────────────────────────────
# Note: RECENT/SEARCH defaults follow 师尊实战偏好, NOT opencode DEFAULTS (5/10).
# They only apply when neither config.json nor env provides a value.
_DEF_API_URL="https://www.mengxy.cc"
_DEF_REQUEST_TIMEOUT="15"        # seconds (curl --max-time)
_DEF_RECENT_COUNT="8"
_DEF_SEARCH_COUNT="8"
_DEF_MAX_CONTENT="3000"
_DEF_MAX_QUERY_LENGTH="200"
_DEF_LOG_DIR="$HOME/.config/cerebro/logs"
_DEF_LOG_ENABLED="1"

# Pre-declare cascade targets so `set -u` is happy before bootstrap runs.
OMEM_API_URL="${OMEM_API_URL:-}"
OMEM_API_KEY="${OMEM_API_KEY:-}"
MEM_REQUEST_TIMEOUT="${MEM_REQUEST_TIMEOUT:-}"
MEM_RECENT_COUNT="${MEM_RECENT_COUNT:-}"
MEM_SEARCH_COUNT="${MEM_SEARCH_COUNT:-}"
MEM_MAX_CONTENT="${MEM_MAX_CONTENT:-}"
MEM_MAX_QUERY_LENGTH="${MEM_MAX_QUERY_LENGTH:-}"
MEM_LOG_DIR="${MEM_LOG_DIR:-}"
MEM_LOG_ENABLED="${MEM_LOG_ENABLED:-}"

# ─── Load shared cerebro config (single source of truth, same as opencode) ───
# Reads $CEREBRO_CONFIG_PATH, else $HOME/.config/cerebro/config.json.
# Populates _CFG_* vars. Silent on missing/parse-error (falls back to defaults).
# Ported from plugins/opencode/src/config.ts loadConfig().
load_cerebro_config() {
  local cfg_path="${CEREBRO_CONFIG_PATH:-$HOME/.config/cerebro/config.json}"
  [[ -z "$cfg_path" || ! -f "$cfg_path" ]] && { log_debug "load_cerebro_config: no config at $cfg_path"; return 0; }
  local line key val
  while IFS=$'\t' read -r key val; do
    [[ -z "$key" ]] && continue
    case "$key" in
      apiUrl)                 _CFG_apiUrl="$val" ;;
      apiKey)                 _CFG_apiKey="$val" ;;
      requestTimeoutMs)       _CFG_requestTimeoutMs="$val" ;;
      maxQueryLength)         _CFG_maxQueryLength="$val" ;;
      maxContentChars)        _CFG_maxContentChars="$val" ;;
      maxContentLength)       _CFG_maxContentLength="$val" ;;
      recentCount)            _CFG_recentCount="$val" ;;
      searchCount)            _CFG_searchCount="$val" ;;
      recentTruncateChars)    _CFG_recentTruncateChars="$val" ;;
      searchTruncateChars)    _CFG_searchTruncateChars="$val" ;;
      recentTimeoutMs)        _CFG_recentTimeoutMs="$val" ;;
      searchTimeoutMs)        _CFG_searchTimeoutMs="$val" ;;
      profileTimeoutMs)       _CFG_profileTimeoutMs="$val" ;;
      autoCaptureThreshold)   _CFG_autoCaptureThreshold="$val" ;;
      ingestMode)             _CFG_ingestMode="$val" ;;
      logEnabled)             _CFG_logEnabled="$val" ;;
      logLevel)               _CFG_logLevel="$val" ;;
      logDir)                 _CFG_logDir="$val" ;;
    esac
  done < <(CEREBRO_CFG_PATH="$cfg_path" python3 -c '
import json, os, sys
path = os.environ["CEREBRO_CFG_PATH"]
try:
    with open(path, "r") as f:
        raw = json.load(f)
except Exception:
    sys.exit(0)
if not isinstance(raw, dict):
    sys.exit(0)
# Flat-config migration (legacy pre-nesting shape)
if "apiUrl" in raw and "connection" not in raw:
    flat = raw
    raw = {"connection": {k: flat[k] for k in ("apiUrl","apiKey","requestTimeoutMs") if k in flat},
           "content":    {k: flat[k] for k in ("maxQueryLength","maxContentChars","maxContentLength") if k in flat},
           "ingest":     {k: flat[k] for k in ("autoCaptureThreshold","ingestMode") if k in flat},
           "logging":    {k: flat[k] for k in ("logEnabled","logLevel","logDir") if k in flat}}

def emit(key, sect, leaf):
    cur = raw.get(sect)
    if isinstance(cur, dict) and leaf in cur and cur[leaf] is not None:
        v = cur[leaf]
        if isinstance(v, bool):
            v = "1" if v else "0"
        print(f"{key}\t{v}")

emit("apiUrl","connection","apiUrl")
emit("apiKey","connection","apiKey")
emit("requestTimeoutMs","connection","requestTimeoutMs")
emit("maxQueryLength","content","maxQueryLength")
emit("maxContentChars","content","maxContentChars")
emit("maxContentLength","content","maxContentLength")
emit("recentCount","injection","recentCount")
emit("searchCount","injection","searchCount")
emit("recentTruncateChars","injection","recentTruncateChars")
emit("searchTruncateChars","injection","searchTruncateChars")
emit("recentTimeoutMs","injection","recentTimeoutMs")
emit("searchTimeoutMs","injection","searchTimeoutMs")
emit("profileTimeoutMs","injection","profileTimeoutMs")
emit("autoCaptureThreshold","ingest","autoCaptureThreshold")
emit("ingestMode","ingest","ingestMode")
emit("logEnabled","logging","logEnabled")
emit("logLevel","logging","logLevel")
emit("logDir","logging","logDir")
' 2>/dev/null)
  log_debug "load_cerebro_config: loaded from $cfg_path"
}

# ─── HTTP Functions (legacy signatures stable; new verbs added) ──────────────

# GET request to cerebro API.
# Usage: omem_get "/v1/memories?limit=20"
omem_get() {
  local path="$1"
  curl -sf --max-time 8 \
    -H "X-API-Key: ${OMEM_API_KEY}" \
    -H "Accept: application/json" \
    "${OMEM_API_URL}${path}" 2>/dev/null || echo '{"error": "request failed"}'
}

# POST request to cerebro API.
# Usage: omem_post "/v1/memories" '{"content": "..."}'
omem_post() {
  local path="$1"
  local body="$2"
  curl -sf --max-time 8 \
    -X POST \
    -H "X-API-Key: ${OMEM_API_KEY}" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "${body}" \
    "${OMEM_API_URL}${path}" 2>/dev/null || echo '{"error": "request failed"}'
}

# PUT request to cerebro API. Uses MEM_REQUEST_TIMEOUT.
# Usage: omem_put "/v1/memories/abc" '{"content": "..."}'
omem_put() {
  local path="$1"
  local body="$2"
  curl -sf --max-time "${MEM_REQUEST_TIMEOUT}" \
    -X PUT \
    -H "X-API-Key: ${OMEM_API_KEY}" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "${body}" \
    "${OMEM_API_URL}${path}" 2>/dev/null || echo '{"error": "request failed"}'
}

# PATCH request to cerebro API. Uses MEM_REQUEST_TIMEOUT.
# Usage: omem_patch "/v1/memories/abc" '{"content": "..."}'
omem_patch() {
  local path="$1"
  local body="$2"
  curl -sf --max-time "${MEM_REQUEST_TIMEOUT}" \
    -X PATCH \
    -H "X-API-Key: ${OMEM_API_KEY}" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "${body}" \
    "${OMEM_API_URL}${path}" 2>/dev/null || echo '{"error": "request failed"}'
}

# DELETE request to cerebro API. Uses MEM_REQUEST_TIMEOUT.
# Usage: omem_delete "/v1/memories/abc"
omem_delete() {
  local path="$1"
  curl -sf --max-time "${MEM_REQUEST_TIMEOUT}" \
    -X DELETE \
    -H "X-API-Key: ${OMEM_API_KEY}" \
    -H "Accept: application/json" \
    "${OMEM_API_URL}${path}" 2>/dev/null || echo '{"error": "request failed"}'
}

# Health probe via /v1/stats. Returns curl exit code (0 = healthy).
omem_health() {
  curl -sf --max-time 5 \
    -H "X-API-Key: ${OMEM_API_KEY}" \
    -H "Accept: application/json" \
    "${OMEM_API_URL}/v1/stats" >/dev/null 2>&1
}

# ─── Input Functions ─────────────────────────────────────────────────────────

# Read hook input JSON from stdin (legacy, kept stable).
# Claude Code pipes hook context as JSON to stdin.
read_stdin() {
  local input=""
  if [[ ! -t 0 ]]; then
    input=$(cat)
  fi
  echo "${input:-"{}"}"
}

# Read hook input JSON from stdin with light schema validation.
# Warns (non-fatal) when transcript_path is absent so silent failures surface.
# Emits the raw JSON on stdout (same shape as read_stdin).
read_hook_input() {
  local input=""
  if [[ ! -t 0 ]]; then
    input=$(cat)
  fi
  input="${input:-"{}"}"
  local has_tp
  has_tp=$(printf '%s' "$input" | python3 -c '
import sys, json
try:
    data = json.load(sys.stdin)
    print("1" if data.get("transcript_path") else "0")
except Exception:
    print("0")
' 2>/dev/null || printf '0')
  if [[ "$has_tp" != "1" ]]; then
    log_warn "read_hook_input: transcript_path missing in hook input"
  fi
  printf '%s\n' "$input"
}

# ─── Project / User Tagging (mirror plugins/opencode/src/tags.ts) ────────────

# Detect git toplevel; fall back to $PWD. Returns empty for home/root dirs
# so we do not tag globally-shared locations as a project.
detect_project_path() {
  local p
  p=$(git rev-parse --show-toplevel 2>/dev/null) || p="$PWD"
  [[ -z "$p" || "$p" == "/" || "$p" == "$HOME" ]] && return 0
  printf '%s\n' "$p"
}

# sha256(input)[:16] — prefers coreutils, falls back to python3.
_sha256_16() {
  local input="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$input" | sha256sum | cut -c1-16
  elif command -v shasum >/dev/null 2>&1; then
    printf '%s' "$input" | shasum -a 256 | cut -c1-16
  else
    printf '%s' "$input" | python3 -c 'import sys,hashlib;print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest()[:16])'
  fi
}

# Emit space-separated container tags: omem_user_<16> and/or omem_project_<16>.
# Mirrors plugins/opencode/src/tags.ts (getUserTag / getProjectTag).
# Email source: OMEM_USER_EMAIL env, else git config user.email.
container_tags() {
  local email project_dir user_tag project_tag
  email="${OMEM_USER_EMAIL:-$(git config user.email 2>/dev/null || true)}"
  project_dir="$(detect_project_path)"
  [[ -n "$email" ]] && user_tag="omem_user_$(_sha256_16 "$email")"
  [[ -n "$project_dir" ]] && project_tag="omem_project_$(_sha256_16 "$project_dir")"
  if [[ -n "$user_tag" && -n "$project_tag" ]]; then
    printf '%s %s\n' "$user_tag" "$project_tag"
  elif [[ -n "$user_tag" ]]; then
    printf '%s\n' "$user_tag"
  elif [[ -n "$project_tag" ]]; then
    printf '%s\n' "$project_tag"
  fi
}

# ─── Content Sanitization (mirror plugins/opencode/src/client.ts) ────────────

# Strip XML-like tag blocks + self-closing tags, collapse whitespace,
# truncate to $1 (default $MEM_MAX_CONTENT). Reads stdin, writes stdout.
# Port of client.ts sanitizeContent().
sanitize_content() {
  local max_len="${1:-$MEM_MAX_CONTENT}"
  MEM_SC_MAX_LEN="$max_len" python3 -c '
import os, re, sys
max_len = int(os.environ.get("MEM_SC_MAX_LEN", "3000"))
data = sys.stdin.read()
# Remove <tag ...>...</tag> blocks (non-greedy, multi-line)
clean = re.sub(r"<[\w-]+[^>]*>[\s\S]*?</[\w-]+>", "", data)
# Remove self-closing tags <tag .../>
clean = re.sub(r"<[\w-]+[^>]*/>", "", clean)
# Collapse whitespace
clean = re.sub(r"\s+", " ", clean).strip()
if len(clean) <= max_len:
    print(clean)
else:
    print(clean[:max_len] + "…[truncated]")
'
}

# Truncate a search query to len (default $MEM_MAX_QUERY_LENGTH).
# Port of client.ts truncateQuery() — no ellipsis, hard slice.
truncate_query() {
  local text="${1:-}"
  local len="${2:-$MEM_MAX_QUERY_LENGTH}"
  [[ -z "$text" ]] && return 0
  if [[ ${#text} -le "$len" ]]; then
    printf '%s' "$text"
  else
    printf '%s' "${text:0:$len}"
  fi
}

# ─── Logging ─────────────────────────────────────────────────────────────────

# Internal: append "<iso-ts> <LEVEL> <msg>" to $MEM_LOG_DIR/claude-code.log.
# Best-effort: never fails the caller (all errors swallowed).
_log() {
  local level="$1"; shift
  [[ "$MEM_LOG_ENABLED" != "1" ]] && return 0
  local msg="$*"
  local ts log_file
  ts="$(date '+%Y-%m-%dT%H:%M:%S%z' 2>/dev/null)" || ts="$(date '+%Y-%m-%dT%H:%M:%S' 2>/dev/null)" || ts="unknown"
  log_file="${MEM_LOG_DIR}/claude-code.log"
  {
    [[ -d "$MEM_LOG_DIR" ]] || mkdir -p "$MEM_LOG_DIR" 2>/dev/null || true
    printf '%s %s %s\n' "$ts" "$level" "$msg" >> "$log_file" 2>/dev/null
  } || true
}
log_warn()  { _log "WARN"  "$*"; }
log_error() { _log "ERROR" "$*"; }
log_debug() { _log "DEBUG" "$*"; }

# ─── Bootstrap (runs at source time) ─────────────────────────────────────────
# Cascade priority: env > config.json > builtin default.
# (matches opencode/src/config.ts: "Env vars have highest priority")
load_cerebro_config   # populates _CFG_* when ~/.config/cerebro/config.json exists

if   [[ -n "${OMEM_API_URL:-}" ]]; then        : ;
elif [[ -n "${_CFG_apiUrl:-}" ]]; then         OMEM_API_URL="${_CFG_apiUrl}";
else                                           OMEM_API_URL="${_DEF_API_URL}"; fi

if   [[ -n "${OMEM_API_KEY:-}" ]]; then        : ;
elif [[ -n "${_CFG_apiKey:-}" ]]; then         OMEM_API_KEY="${_CFG_apiKey}"; fi
# (no builtin default for the key — stays empty when neither source provides it)

# requestTimeout: config stores ms, curl wants seconds.
if   [[ -n "${MEM_REQUEST_TIMEOUT:-}" ]]; then      : ;
elif [[ "${_CFG_requestTimeoutMs:-}" =~ ^[0-9]+$ ]]; then
  MEM_REQUEST_TIMEOUT=$(( _CFG_requestTimeoutMs / 1000 ));
else                                                MEM_REQUEST_TIMEOUT="${_DEF_REQUEST_TIMEOUT}"; fi

if   [[ -n "${MEM_RECENT_COUNT:-}" ]]; then         : ;
elif [[ -n "${_CFG_recentCount:-}" ]]; then         MEM_RECENT_COUNT="${_CFG_recentCount}";
else                                                MEM_RECENT_COUNT="${_DEF_RECENT_COUNT}"; fi

if   [[ -n "${MEM_SEARCH_COUNT:-}" ]]; then         : ;
elif [[ -n "${_CFG_searchCount:-}" ]]; then         MEM_SEARCH_COUNT="${_CFG_searchCount}";
else                                                MEM_SEARCH_COUNT="${_DEF_SEARCH_COUNT}"; fi

if   [[ -n "${MEM_MAX_CONTENT:-}" ]]; then          : ;
elif [[ -n "${_CFG_maxContentLength:-}" ]]; then    MEM_MAX_CONTENT="${_CFG_maxContentLength}";
else                                                MEM_MAX_CONTENT="${_DEF_MAX_CONTENT}"; fi

if   [[ -n "${MEM_MAX_QUERY_LENGTH:-}" ]]; then     : ;
elif [[ -n "${_CFG_maxQueryLength:-}" ]]; then      MEM_MAX_QUERY_LENGTH="${_CFG_maxQueryLength}";
else                                                MEM_MAX_QUERY_LENGTH="${_DEF_MAX_QUERY_LENGTH}"; fi

if   [[ -n "${MEM_LOG_DIR:-}" ]]; then              : ;
elif [[ -n "${_CFG_logDir:-}" ]]; then              MEM_LOG_DIR="${_CFG_logDir}";
else                                                MEM_LOG_DIR="${_DEF_LOG_DIR}"; fi

if   [[ -n "${MEM_LOG_ENABLED:-}" ]]; then          : ;
elif [[ -n "${_CFG_logEnabled:-}" ]]; then          MEM_LOG_ENABLED="${_CFG_logEnabled}";
else                                                MEM_LOG_ENABLED="${_DEF_LOG_ENABLED}"; fi

# Normalize: strip trailing slash from URL, expand leading ~ in logDir.
OMEM_API_URL="${OMEM_API_URL%/}"
MEM_LOG_DIR="${MEM_LOG_DIR/#\~/$HOME}"

# ─── Incremental Cursor (Stop/PreCompact dedup, mirrors supermemory tracker) ─
# Per-session tracker stores the uuid of the last ingested transcript entry.
# Next run only walks entries past that uuid → only the delta is POSTed.
# Tolerant: missing/corrupt file → empty (treat as first run, full sweep).
_CEREBRO_TRACKER_DIR="${HOME}/.config/cerebro/trackers"

# cursor_get <sessionId> → echoes last saved uuid (empty when none/missing).
cursor_get() {
  local sid="${1:-}"
  [[ -z "$sid" ]] && { printf ''; return 0; }
  local f="${_CEREBRO_TRACKER_DIR}/${sid}.txt"
  [[ -f "$f" ]] || { printf ''; return 0; }
  local last
  last=$(head -c 256 "$f" 2>/dev/null | tr -d '\r\n ') || { printf ''; return 0; }
  printf '%s' "$last"
}

# cursor_set <sessionId> <lastId> — persist last id (best-effort, never fails).
cursor_set() {
  local sid="${1:-}" last="${2:-}"
  [[ -z "$sid" || -z "$last" ]] && return 0
  { [[ -d "$_CEREBRO_TRACKER_DIR" ]] || mkdir -p "$_CEREBRO_TRACKER_DIR" 2>/dev/null || true; } || true
  printf '%s\n' "$last" > "${_CEREBRO_TRACKER_DIR}/${sid}.txt" 2>/dev/null || true
}

# ─── Project name detection (mirrors opencode hooks.ts detectProjectName) ────
# Probe AGENTS.md marker is not a name source; derive from manifests:
#   package.json / composer.json (name field), Cargo.toml / pyproject.toml
#   (^name = "..."), go.mod (^module path → last segment). Fallback: dir basename.
# Output: sanitized to [A-Za-z0-9_-], max 32 chars (server cleans anyway).
detect_project_name() {
  local dir
  dir="$(detect_project_path 2>/dev/null)" || dir="$PWD"
  [[ -z "$dir" ]] && dir="$PWD"
  PN_DIR="$dir" python3 -c '
import os, re, json
d = os.environ.get("PN_DIR") or "."
name = ""
for mf, kind in [("package.json","json"), ("composer.json","json"),
                 ("Cargo.toml","toml"), ("pyproject.toml","toml"),
                 ("go.mod","go")]:
    p = os.path.join(d, mf)
    if not os.path.isfile(p): continue
    try:
        txt = open(p, encoding="utf-8").read()
    except Exception:
        continue
    if kind == "json":
        try:
            v = json.loads(txt).get("name")
            if isinstance(v, str) and v:
                name = v; break
        except Exception:
            pass
    elif kind == "toml":
        m = re.search(r"^name\s*=\s*\"([^\"]+)\"", txt, re.M)
        if m: name = m.group(1); break
    elif kind == "go":
        m = re.search(r"^module\s+(\S+)", txt, re.M)
        if m:
            name = m.group(1).rstrip("/").rsplit("/", 1)[-1]; break
if not name:
    name = os.path.basename(d.rstrip("/")) or "project"
name = re.sub(r"[^A-Za-z0-9_-]", "", name)[:32]
sys.stdout.write(name if name else "project")
' 2>/dev/null || printf 'project'
}

# ─── Hook input parsing (shared by Stop + PreCompact) ────────────────────────
# Parse hook stdin JSON → emit "<transcript_path> <session_id>".
# session_id falls back to transcript_path basename (sans extension).
# Uses "-" placeholders so empty fields never collapse together.
hook_paths_from_input() {
  local input="${1:-}"
  HPI_INPUT="$input" python3 -c '
import os, sys, json
try:
    d = json.loads(os.environ.get("HPI_INPUT","") or "{}")
except Exception:
    d = {}
tp = d.get("transcript_path") or ""
sid = d.get("session_id") or d.get("sessionId") or ""
if not sid and tp:
    sid = os.path.splitext(os.path.basename(tp))[0]
print((tp or "-") + " " + (sid or "-"))
' 2>/dev/null || printf '%s\n' "- -"
}

# ─── Session ingest flush (shared by Stop + PreCompact) ──────────────────────
# Usage: flush_session_ingest <transcript_path> <session_id>
#
# Walks transcript JSONL past the saved cursor uuid, filters entries
# (strip system-reminder/cerebro-*/supermemory-* inject-echo blocks, drop
# thinking blocks, truncate tool_result 500 / tool_use input 100, collapse
# whitespace, drop <100-char fragments), POSTs the delta to
# /v1/memories/session-ingest, and advances the cursor only on HTTP 2xx.
# Tolerant: never crashes the caller; returns non-zero on network/HTTP failure
# (cursor NOT advanced → next run retries). Caller should swallow non-zero.
flush_session_ingest() {
  local transcript_path="${1:-}" sid="${2:-}"
  [[ -z "$transcript_path" || ! -f "$transcript_path" ]] && return 0
  [[ -z "$sid" ]] && return 0
  [[ -z "${OMEM_API_KEY:-}" ]] && return 0

  local cursor pn pp
  cursor="$(cursor_get "$sid")"
  pn="$(detect_project_name 2>/dev/null || printf 'project')"
  pp="$(detect_project_path 2>/dev/null || true)"

  # Parse transcript delta → emit "last_uuid\nbody_json".
  # last_uuid  = uuid of the last entry processed (advance cursor here)
  # body_json  = session-ingest body, or empty when all entries were fragments
  # exits 0 with no output when there are zero new entries past the cursor.
  local parsed last_uuid body
  parsed="$(SI_TP="$transcript_path" SI_CURSOR="$cursor" SI_SID="$sid" \
            SI_PN="$pn" SI_PP="$pp" python3 -c '
import os, sys, json, re

tp     = os.environ.get("SI_TP","")
cursor = os.environ.get("SI_CURSOR","")
sid    = os.environ.get("SI_SID","")
pn     = os.environ.get("SI_PN","")
pp     = os.environ.get("SI_PP","")

# Inject-echo blocks we must strip so saved context does not bounce back.
TAG_RE       = re.compile(r"<(system-reminder|cerebro-[a-z0-9_-]+|supermemory-[a-z0-9_-]+)\b[^>]*>[\s\S]*?</\1>", re.I)
SELFCLOSE_RE = re.compile(r"<(system-reminder|cerebro-[a-z0-9_-]+|supermemory-[a-z0-9_-]+)\b[^>]*/>", re.I)
WS_RE        = re.compile(r"\s+")

def clean(s):
    if not isinstance(s, str):
        s = str(s)
    s = TAG_RE.sub("", s)
    s = SELFCLOSE_RE.sub("", s)
    s = WS_RE.sub(" ", s).strip()
    return s

def block_text(b):
    t = b.get("type")
    if t == "text":
        return b.get("text","") or ""
    if t == "thinking":
        return None  # skip reasoning traces
    if t == "tool_result":
        c = b.get("content","")
        if isinstance(c, list):
            parts = []
            for x in c:
                if isinstance(x, dict) and x.get("type") == "text":
                    parts.append(x.get("text","") or "")
                elif isinstance(x, str):
                    parts.append(x)
            c = "\n".join(parts)
        elif not isinstance(c, str):
            try:
                c = json.dumps(c, ensure_ascii=False)
            except Exception:
                c = str(c)
        return "tool_result: " + (c or "")[:500]
    if t == "tool_use":
        inp = b.get("input")
        try:
            inp_s = json.dumps(inp, ensure_ascii=False)
        except Exception:
            inp_s = str(inp)
        return "tool_use(%s): %s" % (b.get("name","?"), inp_s[:100])
    return None

def content_text(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for b in content:
            if isinstance(b, dict):
                bt = block_text(b)
                if bt:
                    parts.append(bt)
        return "\n".join(parts)
    return ""

entries = []  # (uuid, role, raw_text)
try:
    with open(tp, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                d = json.loads(line)
            except Exception:
                continue
            if d.get("type") not in ("user", "assistant"):
                continue
            uid = d.get("uuid","")
            msg = d.get("message")
            if not isinstance(msg, dict):
                continue
            role = msg.get("role")
            if role not in ("user", "assistant"):
                continue
            raw = content_text(msg.get("content"))
            entries.append((uid, role, raw))
except Exception:
    entries = []

# Locate cursor: entries past it are the delta.
# If the cursor uuid is not found (e.g. transcript rewritten after compact),
# fall back to full sweep — safe (server dedups via customId/session_id).
start = 0
if cursor:
    for i, (u, _, _) in enumerate(entries):
        if u == cursor:
            start = i + 1
            break

new = entries[start:]
if not new:
    sys.exit(0)  # nothing past cursor → no output → no cursor change

last_uid = new[-1][0]
out = []
for uid, role, raw in new:
    txt = clean(raw)
    if len(txt) < 100:   # drop fragments
        continue
    out.append({"role": role, "content": txt})

body = {
    "messages": out,
    "agent_id": os.environ.get("OMEM_AGENT_ID", "claude-code"),
}
if sid: body["session_id"]      = sid
if pn: body["project_name"]     = pn
if pp: body["project_path"]     = pp

sys.stdout.write(last_uid + "\n")
if out:
    sys.stdout.write(json.dumps(body, ensure_ascii=False))
' 2>/dev/null)" || parsed=""

  if [[ -z "$parsed" ]]; then
    return 0  # no new entries past cursor
  fi
  last_uuid="${parsed%%$'\n'*}"
  if [[ "$parsed" == *$'\n'* ]]; then
    body="${parsed#*$'\n'}"
  else
    body=""
  fi
  [[ -z "$last_uuid" ]] && return 0

  if [[ -n "$body" ]]; then
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 25 \
      -X POST \
      -H "X-API-Key: ${OMEM_API_KEY}" \
      -H "Content-Type: application/json" \
      -H "Accept: application/json" \
      -d "$body" \
      "${OMEM_API_URL}/v1/memories/session-ingest" 2>/dev/null) || http_code="000"
    if [[ "$http_code" =~ ^2 ]]; then
      cursor_set "$sid" "$last_uuid"
      log_debug "flush_session_ingest: ok http=$http_code cursor=$last_uuid"
      return 0
    else
      log_error "flush_session_ingest: http=$http_code (cursor NOT advanced, will retry next run)"
      return 1
    fi
  else
    # All new entries were <100-char fragments — nothing to send, but still
    # advance the cursor so we do not re-parse these on every future run.
    cursor_set "$sid" "$last_uuid"
    log_debug "flush_session_ingest: 0 msgs kept (fragments), advancing cursor=$last_uuid"
    return 0
  fi
}
