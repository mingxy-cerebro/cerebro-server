# Cerebro Server (omem-server-source)

主 agent 文档：`AGENTS.md`（OpenCode 专用，勿混写）。Claude Code 工程技能配置如下。

## 插件发布铁律（改 plugins/ 必读）

改动 `plugins/*/` 任何子包代码后，发布三件套缺一不可：

1. **版本双 bump**：`package.json` 必 bump；claude-code/zcode 还有 `.claude-plugin/plugin.json`，两处版本必须一致（两个文件不同步 = 版本黑洞）。
2. **npm publish**：五个包全是公开 npm 包（`@mingxy/cerebro-claude-code` / `@mingxy/cerebro-mcp` / `@ourmem/ourmem` / `@mingxy/cerebro` / `@mingxy/cerebro-zcode`），git push 后必须 `npm publish`（有 build 步骤的包先 `npm run build`，mcp 包 prepublishOnly 自动跑 tsc）。publish 失败不算发布完成。
3. **CC 插件缓存同步**：本机实际在用的插件缓存目录（`~/.claude/plugins/cache/cerebro/cerebro/<ver>/`，以 `.in_use` 为准，孤儿版本目录不算数）要手动覆盖新文件——重装走 marketplace 有延迟，直接 cp 立即生效。

## 服务端部署铁律

部署动向前**直接 loadskill `omem-iteration`**（`.claude/skills/omem-iteration/SKILL.md`）——三端部署规则、生产机信息、验收清单全在该 skill 里，勿凭记忆动手、勿在本文件复制细节（两处维护必脱节）。git commit + push 必须在部署之前。

## Agent skills

### Issue tracker

Issues live in GitHub Issues (`mingxy-cerebro/cerebro-server`), worked via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-label vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout — root `CONTEXT.md` + `docs/adr/`, created lazily by `/domain-modeling`. See `docs/agents/domain.md`.
