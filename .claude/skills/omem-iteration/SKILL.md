---
name: omem-iteration
description: 月儿OMEM迭代管理工作流。Keywords: 迭代管理、项目进度、handoff、omem任务、路线图、三端部署、publish、上线、新会话恢复上下文、deploy, release, iteration roadmap.
---

# omem-iteration · Iteration & Deployment Workflow

## Iron rules

1. Seal before ascend — git commit + push ALWAYS before deploy
2. Evidence-driven — no "done" without a passing build; no "deployed" without MD5 match
3. Personally verify disciple work; big changes get an oracle review; P0 blocks everything until fixed
4. Active memory writes go to CC local md (`~/.claude/projects/<proj>/memory/`), NEVER memory_store (session-ingest auto-collects; double-writing = duplicates)
5. Roadmap/progress lives in GitHub Issues (mingxy-cerebro/cerebro-server) — this skill holds no progress snapshots

## Flow

Restore context → analyze → diagnose root cause → design → delegate → verify + oracle review → seal (commit) → deploy → persist memory

1. **Context**: read the session-start [CEREBRO-MEMORY] injection (profile + topic clusters) first; only memory_search for what's missing (past decisions, pitfalls). No blanket 5-query sweeps.
2. **Analyze**: P0 bleeding → act now; P1 → today; P2 → plan first. Complex intent → metis; architecture → oracle.
3. **Diagnose**: run commands before conclusions; logs + code + data; fix root cause, not symptom.
4. **Delegate** (CC Task tool, real agent names):

| Agent | Name | Purpose |
|-------|------|---------|
| metis | Lingxi | requirements analysis (read-only) |
| oracle | Xuanji | architecture review |
| momus | Mingjing | code review |
| Explore | Tanxu | code search & locate |

Delegation brief must carry: TASK / EXPECTED OUTCOME / REQUIRED TOOLS / MUST DO / MUST NOT DO / CONTEXT (paths + constraints).

## Verify (after every disciple delivery)

- Diff check: missing imports, unused vars, `AppState` field changes must sync 4 sites (server.rs / main.rs / api/mod.rs / stats.rs); ban `as any` / `@ts-ignore` / `unwrap()`
- Zero-warning build: Rust `cargo build -p omem-server`; TS `npx tsc --noEmit`
- Actually run it: curl the API, check journalctl, view the frontend

**Oracle review** triggers (any): >3 files / core logic (store, ingest, lifecycle) / new API / security / performance / uncertain. Skip for docs-only, typos, pure styling. P0 must be fixed before proceeding; skipping P1 requires a recorded reason.

## Git

Chinese commit message, format `<type>(<scope>): 描述`. Push via WSL git ssh remote (direct). A 443 timeout means you slipped into Windows git https remote — switch back to WSL ssh remote. Run `codegraph sync` after committing.

## Deployment (three targets)

### Server (Rust) — build → atomic swap → verify

```bash
# 1. Build (external target-dir — artifact is NOT in repo target/)
cargo build --release
# Artifact: /mnt/d/dev/github/project/omem-server-build/release/omem-server

# 2. Transport: real WSL connects directly with ed25519 (passwordless).
#    In-sandbox port 22 is blocked — Bash with dangerouslyDisableSandbox escapes
#    the sandbox, or use windows-mcp → wsl.exe (real WSL). socat 3128 proxy
#    ticket expired — dead channel, do not use.

# 3. Atomic swap + MD5 both ends (writing the live binary directly gets EBUSY — must .new + mv)
BIN=/mnt/d/dev/github/project/omem-server-build/release/omem-server
md5sum $BIN
scp $BIN root@47.93.199.242:/opt/omem/omem-server.new
ssh root@47.93.199.242 "md5sum /opt/omem/omem-server.new && mv /opt/omem/omem-server.new /opt/omem/omem-server"

# 4. Restart + verify
ssh root@47.93.199.242 "systemctl restart omem && sleep 2 && systemctl status omem"
ssh root@47.93.199.242 "journalctl -u omem --since '30 seconds ago' --no-pager | tail -20"
curl https://www.mengxy.cc/health   # expect 200
```

### Frontend (omem-web)

The panel is **embedded in each plugin's `web/` dir** (served locally by the plugin's web-server, port 5212) — nginx does NOT serve it (no root points at /var/www/omem-web; scp there is a dead end).

```bash
cd omem-web && npm run build
# then copy dist/* into each plugin's web/ dir and go through the plugin release trio
cp -r dist/* ../plugins/claude-code/web/ && cp -r dist/* ../plugins/opencode/web/ && cp -r dist/* ../plugins/zcode/web/
```
(scripts/build-plugin-web.sh covers opencode only)

### Plugin release trio (mandatory after touching plugins/)

1. **Double version bump**: package.json AND `.claude-plugin`/`.zcode-plugin` plugin.json must match
2. **npm publish**: `npm publish --access public` (build first if the package has a build step; a failed publish = not released)
3. **Cache sync**: the live claude-code cache is the `~/.claude/plugins/cache/cerebro/cerebro/<ver>/` dir containing `.in_use` — `/plugin update` or manually cp new files into the in_use dir for instant effect; opencode: delete `~/.cache/opencode/packages/@mingxy/cerebro@latest` to force re-pull

npm hits EROFS inside the sandbox — run with dangerouslyDisableSandbox.

## Server profile

- SSH `root@47.93.199.242`, domain `https://www.mengxy.cc`
- Binary `/opt/omem/omem-server` (backups as .bak-*); data `/opt/omem/omem-data/`; config `/opt/omem/.env` (scheduler 12h)
- Nginx: `/etc/nginx/sites-enabled/www.mengxy.cc.conf`
- **Tiny box: 3.4Gi RAM** — heavy jobs (full-table scans, index retraining, multi-space sweeps) can OOM it and kill sshd. See LanceDB section below.
- API auth `X-API-Key: {tenant_id}` — live key in `~/.config/cerebro/config.json` (never commit)
- Embedding `Pro/BAAI/bge-m3` + reranker `Pro/BAAI/bge-reranker-v2-m3` (SiliconFlow)

## LanceDB ops (2026-08-15 world model — READ THIS, it's the OOM root cause)

- **Version model**: every add/delete/update creates a new version (manifest + data files). GC deletes files but NEVER reclaims version numbers. "Version explosion" was the root cause of the old OOM.
- **Three-layer GC defense**: ① write-path after_mutation (GC_VERSION_THRESHOLD=30, background prune+compact+index merge) ② maybe_optimize (>50 versions) ③ scheduler full sweep (every 12h) + optimize_all_on_disk at startup
- optimize() = Compact + Prune(older_than=0) + Index merge
- **8/12 incident postmortem**: scheduler swept 99 spaces every 30 min — each a full-table scan + drop/retrain of 7 indices = rebuild storm. 3.4Gi box filled to 93%, sshd got killed (the real reason WSL couldn't connect that day).
- **2026-08-15 surgery** (commit d72358c): init_table dropped purge+rebuild_indices (cold init 11s→25ms), global lock removed from get_store, LRU 20→128, scheduler dropped to 12h via .env. rebuild_indices remains a manual maintenance tool.
- **Daily watch**: `_indices` dir count stable (personal space: 7), personal memories.lance ~6.6M, no memory spikes on the hour.

## Emergency playbook

- Slow requests: `journalctl -u omem | grep 'request completed'` for duration_ms (server-side timing is authoritative). Past culprits: lock-held init (fixed), rebuild storm (fixed), LanceDB fragmentation (GC-managed)
- Build failure: try package-level `cargo build -p omem-server` first; a new struct field must sync every initializer
- Plugin update not taking effect: check in_use cache dir / opencode packages cache
- Weird sandbox networking (proxy envs, fake listeners, UDP blocked): compare from outside the sandbox before diagnosing — sandbox phantoms cause misdiagnosis
