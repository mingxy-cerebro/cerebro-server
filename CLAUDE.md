# Cerebro Server (omem-server-source)

主 agent 文档：`AGENTS.md`（OpenCode 专用，勿混写）。Claude Code 工程技能配置如下。

## Agent skills

### Issue tracker

Issues live in GitHub Issues (`mingxy-cerebro/cerebro-server`), worked via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-label vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout — root `CONTEXT.md` + `docs/adr/`, created lazily by `/domain-modeling`. See `docs/agents/domain.md`.
