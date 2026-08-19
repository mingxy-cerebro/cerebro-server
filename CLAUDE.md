# Cerebro Server (omem-server-source)

主 agent 文档：`AGENTS.md`（OpenCode 专用，勿混写）。Claude Code 工程技能配置如下。

## 插件发布铁律（改 plugins/ 必读）

改动 `plugins/*/` 任何子包代码后，发布三件套缺一不可：

1. **版本双 bump**：`package.json` 必 bump；claude-code/zcode 还有 `.claude-plugin/plugin.json`，两处版本必须一致（两个文件不同步 = 版本黑洞）。
2. **npm publish**：五个包全是公开 npm 包（`@mingxy/cerebro-claude-code` / `@mingxy/cerebro-mcp` / `@ourmem/ourmem` / `@mingxy/cerebro` / `@mingxy/cerebro-zcode`），git push 后必须 `npm publish`（有 build 步骤的包先 `npm run build`，mcp 包 prepublishOnly 自动跑 tsc）。publish 失败不算发布完成。
3. **CC 插件缓存同步**：本机实际在用的插件缓存目录（`~/.claude/plugins/cache/cerebro/cerebro/<ver>/`，以 `.in_use` 为准，孤儿版本目录不算数）要手动覆盖新文件——重装走 marketplace 有延迟，直接 cp 立即生效。

## 服务端部署铁律（部署动向前必读）

部署规则的**权威源是 skill**：`.claude/skills/omem-iteration/SKILL.md`（七重天·三端飞升 + 仙境地图）。部署前先加载该 skill，勿凭记忆摸黑。要点速查：

- **生产机**：`root@47.93.199.242`（= www.mengxy.cc）。ssh config 里的 svr3（39.96.6.152）**不是** omem 生产机。密钥 `~/.ssh/id_ed25519`（真实 WSL 免密直通）。
- **顺序**：git commit + push 必须在部署之前。
- **流程**：WSL `cargo build --release`（产物在外置 target-dir `/mnt/d/dev/github/project/omem-server-build/release/omem-server`，repo 根 `.cargo/config.toml` 指定，不在 repo 的 target/ 下）→ `scp` 到 `/opt/omem/omem-server.new` → 双端 `md5sum` 校验一致 → `cp 旧件 .old && mv .new 正名` 原子换（直写运行中二进制会被锁拒）→ `systemctl restart omem` → `systemctl is-active omem` + `curl https://www.mengxy.cc/health` + `journalctl -u omem` 验收。
- **通道**：Bash 沙箱只读/断网时走 windows-mcp→wsl.exe 进真实 WSL 直连（skill 里 socat 3128 代理通道已过期弃用）。
- **渠道坑**：windows-mcp PowerShell 发 wsl.exe 时，命令里 `$VAR` 会被 PowerShell 插值吃掉——用全路径，不用 shell 变量。

## Agent skills

### Issue tracker

Issues live in GitHub Issues (`mingxy-cerebro/cerebro-server`), worked via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-label vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout — root `CONTEXT.md` + `docs/adr/`, created lazily by `/domain-modeling`. See `docs/agents/domain.md`.
