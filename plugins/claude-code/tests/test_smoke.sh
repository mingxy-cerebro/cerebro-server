#!/usr/bin/env bash
# cerebro Claude Code plugin — Phase5 smoke tests
#
# 零依赖：纯 bash + python3（不装 bats）。一键跑：bash tests/test_smoke.sh
#
# 覆盖：
#   - session-start：无 key 分支（[cerebro] 提示）+ 带 key 分支（stub server，
#     断言 [CEREBRO-MEMORY] / [CEREBRO-TIME] / <cerebro-profile> / recent）
#   - user-prompt-submit：保存类 / 召回类 / 普通 / 英文 关键词 nudge
#   - recall-approve：memory-search 放行 / 普通 Bash / 危险 Bash
#   - memory-save / memory-search：参数校验 exit code + --help
#   - stop / pre-compact：无 key 短路 {}
set -uo pipefail

PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOOKS="$PLUGIN_DIR/hooks"
SCRIPTS="$PLUGIN_DIR/scripts"
TMP="$(mktemp -d)"
STUB_PID=""

cleanup() {
  [[ -n "$STUB_PID" ]] && kill "$STUB_PID" 2>/dev/null || true
  rm -rf "$TMP" 2>/dev/null || true
}
trap cleanup EXIT

# ─── 环境隔离：不读真实 config，不带真实 key，不写日志 ────────────────────────
export CEREBRO_CONFIG_PATH="/nonexistent-test-cfg.json"
export OMEM_API_KEY=""
export OMEM_API_URL="http://127.0.0.1:1"   # unreachable → curl fails silently
export MEM_LOG_ENABLED="0"
export MEM_LOG_DIR="$TMP/logs"

PASS=0
FAIL=0
FAILED_CASES=()

ok()   { echo "PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "FAIL: $1"; FAIL=$((FAIL + 1)); FAILED_CASES+=("$1"); }

assert_contains()     { # <label> <haystack> <needle>
  if [[ "$2" == *"$3"* ]]; then ok "$1"; else fail "$1 (missing: $3)"; fi
}
assert_not_contains() { # <label> <haystack> <needle>
  if [[ "$2" != *"$3"* ]]; then ok "$1"; else fail "$1 (unexpected: $3)"; fi
}
assert_exit()         { # <label> <expected> <actual>
  if [[ "$2" == "$3" ]]; then ok "$1"; else fail "$1 (exit: want=$2 got=$3)"; fi
}
assert_eq()           { # <label> <expected> <actual>
  if [[ "$2" == "$3" ]]; then ok "$1"; else fail "$1 (want=[$2] got=[$3])"; fi
}

# ─── 本地 stub HTTP server（测 session-start 带 key 分支）─────────────────────
# 返回固定 profile + recent JSON，让 hook 走 [CEREBRO-MEMORY] 组装分支。
start_stub() {
  cat > "$TMP/stub.py" <<'PYEOF'
import http.server, socketserver, json
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        p = self.path
        if p.startswith("/v2/profile/inject"):
            body = json.dumps({"content": "TEST PROFILE", "preference_count": 1})
        elif p.startswith("/v1/memories"):
            body = json.dumps({"memories": [{"content": "recent activity line", "id": "m1"}]})
        else:
            body = "{}"
        b = body.encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)
    def log_message(self, *a): pass
class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
srv = S(("127.0.0.1", 0), H)
print(srv.server_address[1], flush=True)
srv.serve_forever()
PYEOF
  python3 "$TMP/stub.py" > "$TMP/port" 2>/dev/null &
  STUB_PID=$!
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    [[ -s "$TMP/port" ]] && break
    sleep 0.1
  done
  STUB_PORT="$(cat "$TMP/port" 2>/dev/null || echo "")"
}

echo "=== SessionStart ==="

# (1) 无 key 分支
out="$(OMEM_API_KEY="" bash "$HOOKS/session-start.sh" </dev/null 2>/dev/null || true)"
assert_contains "session-start no-key: hookSpecificOutput" "$out" '"hookSpecificOutput"'
assert_contains "session-start no-key: [cerebro] prefix"    "$out" '[cerebro]'
assert_contains "session-start no-key: mentions API key"    "$out" 'OMEM_API_KEY'

# (2) 带 key 分支（stub server）——验证 [CEREBRO-MEMORY] / [CEREBRO-TIME] / profile / recent
start_stub
if [[ -z "$STUB_PORT" ]]; then
  fail "session-start stub: server did not start"
else
  out="$(OMEM_API_KEY="test-key" OMEM_API_URL="http://127.0.0.1:$STUB_PORT" \
         bash "$HOOKS/session-start.sh" </dev/null 2>/dev/null || true)"
  assert_contains "session-start stub: hookSpecificOutput"   "$out" '"hookSpecificOutput"'
  assert_contains "session-start stub: [CEREBRO-MEMORY] open" "$out" '[CEREBRO-MEMORY]'
  assert_contains "session-start stub: [/CEREBRO-MEMORY] close" "$out" '[/CEREBRO-MEMORY]'
  assert_contains "session-start stub: [CEREBRO-TIME] line"  "$out" '[CEREBRO-TIME]'
  assert_contains "session-start stub: time prefix text"     "$out" '当前:'
  assert_contains "session-start stub: <cerebro-profile>"    "$out" '<cerebro-profile>'
  assert_contains "session-start stub: profile content"      "$out" 'TEST PROFILE'
  assert_contains "session-start stub: recent activity"      "$out" 'recent activity line'
fi

echo
echo "=== UserPromptSubmit (keyword nudge) ==="

run_ups() { printf '%s' "$1" | bash "$HOOKS/user-prompt-submit.sh" 2>/dev/null || true; }

# 保存类（中文）
out="$(run_ups '{"prompt":"请记住这个决定：以后用 rust 写后端"}')"
assert_contains "ups save-zh: nudge tag"     "$out" '<cerebro-nudge>'
assert_contains "ups save-zh: memory-save"   "$out" 'memory-save skill'
assert_contains "ups save-zh: recall instr"  "$out" '<cerebro-recall>'

# 召回类（中文）
out="$(run_ups '{"prompt":"上次那个 bug 修好了吗"}')"
assert_contains "ups recall-zh: nudge tag"   "$out" '<cerebro-nudge>'
assert_contains "ups recall-zh: memory-search" "$out" 'memory-search skill'

# 保存类（英文）
out="$(run_ups '{"prompt":"please remember this decision"}')"
assert_contains "ups save-en: nudge tag"     "$out" '<cerebro-nudge>'
assert_contains "ups save-en: memory-save"   "$out" 'memory-save skill'

# 召回类（英文）
out="$(run_ups '{"prompt":"lets continue from earlier"}')"
assert_contains "ups recall-en: nudge tag"   "$out" '<cerebro-nudge>'
assert_contains "ups recall-en: memory-search" "$out" 'memory-search skill'

# 普通消息：无 nudge，只有 recall 指令
out="$(run_ups '{"prompt":"你好，今天天气如何"}')"
assert_not_contains "ups plain: no nudge"    "$out" '<cerebro-nudge>'
assert_contains     "ups plain: recall instr" "$out" '<cerebro-recall>'

# 空/缺字段 stdin：不崩，有 recall 指令
out="$(run_ups '{}')"
assert_contains "ups empty: recall instr"    "$out" '<cerebro-recall>'
assert_not_contains "ups empty: no nudge"    "$out" '<cerebro-nudge>'

echo
echo "=== recall-approve (PreToolUse) ==="

run_ra() { printf '%s' "$1" | bash "$HOOKS/recall-approve.sh" 2>/dev/null || true; }

# memory-search Skill → allow
out="$(run_ra '{"tool_name":"Skill","tool_input":{"name":"memory-search"}}')"
assert_contains "ra skill-search: allow"     "$out" '"permissionDecision"'
assert_contains "ra skill-search: allow val" "$out" 'allow'

# tool_name 直接含 memory-search → allow
out="$(run_ra '{"tool_name":"memory-search","tool_input":{}}')"
assert_contains "ra direct-search: allow"    "$out" 'allow'

# 普通 Bash（非 memory-search）→ {}
out="$(run_ra '{"tool_name":"Bash","tool_input":{"command":"ls -la"}}')"
assert_eq "ra plain-bash: deny {}"           '{}' "$out"

# 危险 Bash（memory-search.sh; rm）→ {}
out="$(run_ra '{"tool_name":"Bash","tool_input":{"command":"bash memory-search.sh; rm -rf /"}}')"
assert_eq "ra danger-bash: deny {}"          '{}' "$out"

# 危险 Bash（$( 命令替换）→ {}
out="$(run_ra '{"tool_name":"Bash","tool_input":{"command":"memory-search.sh $(whoami)"}}')"
assert_eq "ra subst-bash: deny {}"           '{}' "$out"

echo
echo "=== memory-save arg validation ==="

# 空 content → exit 1
bash "$SCRIPTS/memory-save.sh" </dev/null >/dev/null 2>&1; rc=$?
assert_exit "save empty-content: exit 1"     "1" "$rc"

# bad visibility → exit 2
bash "$SCRIPTS/memory-save.sh" --content "x" --visibility bad >/dev/null 2>&1; rc=$?
assert_exit "save bad-visibility: exit 2"    "2" "$rc"

# bad category → exit 2
bash "$SCRIPTS/memory-save.sh" --content "x" --category bad >/dev/null 2>&1; rc=$?
assert_exit "save bad-category: exit 2"      "2" "$rc"

# bad scope → exit 2
bash "$SCRIPTS/memory-save.sh" --content "x" --scope bad >/dev/null 2>&1; rc=$?
assert_exit "save bad-scope: exit 2"         "2" "$rc"

# unknown option → exit 2
bash "$SCRIPTS/memory-save.sh" --bogus >/dev/null 2>&1; rc=$?
assert_exit "save unknown-opt: exit 2"       "2" "$rc"

# --help → exit 0 + usage text
help_out="$(bash "$SCRIPTS/memory-save.sh" --help 2>&1 || true)"; rc=$?
assert_exit "save --help: exit 0"            "0" "$rc"
assert_contains "save --help: usage text"    "$help_out" 'content'

echo
echo "=== memory-search arg validation ==="

# 空 query → exit 1
bash "$SCRIPTS/memory-search.sh" "" </dev/null >/dev/null 2>&1; rc=$?
assert_exit "search empty-query: exit 1"     "1" "$rc"

echo
echo "=== stop / pre-compact (no-key short-circuit) ==="

# stop 无 key → {}
out="$(OMEM_API_KEY="" bash "$HOOKS/stop.sh" </dev/null 2>/dev/null || true)"
assert_eq "stop no-key: {}"                  '{}' "$out"

# pre-compact 无 key → {}
out="$(OMEM_API_KEY="" bash "$HOOKS/pre-compact.sh" </dev/null 2>/dev/null || true)"
assert_eq "pre-compact no-key: {}"           '{}' "$out"

echo
echo "========================"
TOTAL=$((PASS + FAIL))
echo "Total: $TOTAL | PASS: $PASS | FAIL: $FAIL"
if [[ "$FAIL" -ne 0 ]]; then
  echo "Failed cases:"
  for c in "${FAILED_CASES[@]}"; do echo "  - $c"; done
  exit 1
fi
echo "ALL PASS"
exit 0
